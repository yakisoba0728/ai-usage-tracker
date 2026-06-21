import { type ReactNode } from "react";

import { cn } from "@/lib/utils";

export function InfoLine({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <div className="mb-1 text-xs text-text-faint">{label}</div>
      <div className={cn("truncate text-sm text-text", mono && "num")}>{value}</div>
    </div>
  );
}

export function MenuItem({
  icon,
  children,
  onClick,
  disabled = false,
  destructive = false,
}: {
  icon: ReactNode;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-white/[0.06] disabled:opacity-40",
        destructive ? "text-crit" : "text-text-dim hover:text-text",
      )}
    >
      {icon}
      {children}
    </button>
  );
}
