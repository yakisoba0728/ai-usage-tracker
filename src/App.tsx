import { lazy, Suspense } from "react";

/**
 * The dashboard (search, cards, detail modal, settings) is the app's only
 * webview window. The menu-bar surface is a native macOS tray menu (built in
 * Rust), not a webview — so there is no popover window here.
 */
const Dashboard = lazy(() =>
  import("@/components/Dashboard").then((m) => ({ default: m.Dashboard })),
);

export default function App() {
  return (
    <Suspense fallback={null}>
      <Dashboard />
    </Suspense>
  );
}
