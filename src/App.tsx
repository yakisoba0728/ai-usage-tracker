import { lazy, Suspense } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { PopoverShell } from "@/components/PopoverShell";

/**
 * Both webviews render the full dashboard. The borderless tray popover is the
 * primary surface — clicking the menu-bar icon drops it down with usage right
 * there; the (tray-menu-opened) main window shows the same dashboard plain.
 */
const Dashboard = lazy(() =>
  import("@/components/Dashboard").then((m) => ({ default: m.Dashboard })),
);

/**
 * Which webview is this? The backend loads the popover as
 * `index.html?window=popover` in a window labeled "popover". The query is
 * authoritative in dev; the Tauri label confirms it in the built app. Outside
 * Tauri (`pnpm dev`) this resolves to the plain dashboard.
 */
function isPopoverWindow(): boolean {
  if (typeof window === "undefined") return false;
  if (new URLSearchParams(window.location.search).get("window") === "popover") {
    return true;
  }
  if ("__TAURI_INTERNALS__" in window) {
    try {
      return getCurrentWindow().label === "popover";
    } catch {
      return false;
    }
  }
  return false;
}

const isPopover = isPopoverWindow();

export default function App() {
  return (
    <Suspense fallback={null}>
      {isPopover ? (
        <PopoverShell>
          <Dashboard />
        </PopoverShell>
      ) : (
        <Dashboard />
      )}
    </Suspense>
  );
}
