import { lazy, Suspense } from "react";

/**
 * Both webviews — the borderless tray popover and the (tray-menu-opened) main
 * window — render the full dashboard. The tray popover is the primary surface:
 * clicking the menu-bar icon shows usage right there.
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
