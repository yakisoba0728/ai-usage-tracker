import type { ServiceUsage } from "@/lib/types";

export type InspectorTab = "limits" | "sessions" | "raw" | "settings";

export const INSPECTOR_TABS: InspectorTab[] = [
  "limits",
  "sessions",
  "raw",
  "settings",
];

export function inspectorTabsForService(
  service: Pick<ServiceUsage, "raw_response">,
): InspectorTab[] {
  if (service.raw_response) return INSPECTOR_TABS;
  return INSPECTOR_TABS.filter((id) => id !== "raw");
}
