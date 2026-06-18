import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AppConfig, UsageSnapshot } from "@/lib/types";

const USAGE_UPDATED_EVENT = "usage-updated";

export function getUsage(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("get_usage");
}

export function refreshNow(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("refresh_now");
}

export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

export function setConfig(next: AppConfig): Promise<void> {
  return invoke<void>("set_config", { new: next });
}

/**
 * Subscribe to backend `usage-updated` pushes. Returns a promise that resolves
 * to an unlisten function once the listener is attached.
 */
export function onUsageUpdated(
  cb: (snapshot: UsageSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<UsageSnapshot>(USAGE_UPDATED_EVENT, (event) => {
    cb(event.payload);
  });
}
