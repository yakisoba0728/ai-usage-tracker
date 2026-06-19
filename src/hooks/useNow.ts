import { useEffect, useState } from "react";

/**
 * A re-rendering `now` (ms) that ticks on an interval (1s by default).
 * Lift one instance to the top of the tree and pass `nowMs` down so every
 * live countdown / "updated Xs ago" label stays fresh from a single timer.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}
