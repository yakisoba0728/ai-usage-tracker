/**
 * Toggle a notification threshold (%) in a provider's `notify_thresholds` list.
 *
 * Adds `value` when absent, removes it when present, and always returns a new
 * array that is sorted ascending NUMERICALLY (JS `.sort()` is lexical by
 * default, which mis-orders e.g. [100, 50, 75]) and deduped. Pure: the input is
 * never mutated.
 */
export function toggleThreshold(thresholds: number[], value: number): number[] {
  const present = thresholds.includes(value);
  const next = present
    ? thresholds.filter((t) => t !== value)
    : [...thresholds, value];
  return Array.from(new Set(next)).sort((a, b) => a - b);
}
