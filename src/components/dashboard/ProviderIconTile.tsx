import { ProviderMark } from "@/components/ProviderMark";
import { statusFillClass } from "@/components/dashboard/helpers";
import type { ServiceStatus } from "@/lib/status";
import type { Provider } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Provider glyph in a rounded tile with a status dot. Shared by the account
 * card and the detail-modal header (`large`). */
export function ProviderIconTile({
  provider,
  status,
  large = false,
}: {
  provider: Provider;
  status: ServiceStatus;
  large?: boolean;
}) {
  return (
    <span
      className={cn(
        "relative flex shrink-0 items-center justify-center rounded-md border border-border bg-surface-2 text-text-dim",
        large ? "size-11" : "size-9",
      )}
    >
      <ProviderMark provider={provider} className={large ? "size-6" : "size-5"} />
      {status !== "offline" && (
        <span className={cn("absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-surface-2", statusFillClass(status))} />
      )}
    </span>
  );
}
