import { cn } from "@/lib/utils";

/**
 * Connection state indicator: a solid green dot with a soft halo when
 * connected, a muted gray dot when offline. Purely decorative.
 */
export function StatusDot({
  connected,
  className,
}: {
  connected: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "relative flex size-2.5 shrink-0 items-center justify-center",
        className,
      )}
    >
      <span
        className={cn(
          "absolute inline-flex size-2.5 rounded-full",
          connected ? "bg-ok/20" : "bg-muted-foreground/15",
        )}
      />
      <span
        className={cn(
          "relative inline-flex size-1.5 rounded-full",
          connected ? "bg-ok" : "bg-muted-foreground/50",
        )}
      />
    </span>
  );
}
