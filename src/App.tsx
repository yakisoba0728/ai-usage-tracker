import { getCurrentWindow } from "@tauri-apps/api/window";

import { Dashboard } from "@/components/Dashboard";
import { TrayPopover } from "@/components/TrayPopover";

/**
 * Which window is this webview? The backend loads the tray popover as
 * `index.html?window=popover` in a borderless window labeled "popover"; the
 * main dashboard is the default. We detect both ways (URL query is authoritative
 * in dev, the Tauri label confirms it in the built app). Outside Tauri (plain
 * `pnpm dev`) this resolves to the dashboard.
 */
function isPopoverWindow(): boolean {
  if (typeof window === "undefined") return false;
  if (
    new URLSearchParams(window.location.search).get("window") === "popover"
  ) {
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

// Resolved once at import — the window identity never changes mid-session.
const isPopover = isPopoverWindow();

export default function App() {
  return isPopover ? <TrayPopover /> : <Dashboard />;
}
