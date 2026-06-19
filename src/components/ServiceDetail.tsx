import { useEffect, useState } from "react";
import { AlertCircle, Wifi, WifiOff } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import {
  formatReset,
  formatUsedLimit,
  percentBarColor,
  percentColor,
} from "@/lib/format";
import type { LimitWindow, Provider, ServiceUsage } from "@/lib/types";

const SOURCE_NOTE: Record<Provider, string> = {
  claude: "via Claude Code OAuth · api.anthropic.com",
  codex: "via Codex CLI · chatgpt.com/backend-api/wham/usage",
  gemini: "via Gemini CLI · Google Code Assist",
  copilot: "via Copilot CLI · api.github.com/copilot_internal/user",
  cursor: "via Cursor · api2.cursor.sh (experimental)",
};

function DetailRow({ w, nowMs }: { w: LimitWindow; nowMs: number }) {
  const pct = w.used_percent;
  const usedLimit = formatUsedLimit(w);
  const reset = formatReset(w.resets_at, nowMs);
  return (
    <div className="rounded-md border bg-muted/30 p-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">{w.label}</span>
        <span className={cn("text-lg font-semibold tabular-nums", percentColor(pct))}>
          {pct == null ? "—" : `${pct.toFixed(pct < 10 ? 1 : 0)}%`}
        </span>
      </div>
      <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className={cn("h-full rounded-full transition-all", percentBarColor(pct))}
          style={{ width: `${Math.min(100, pct ?? 0)}%` }}
        />
      </div>
      {(usedLimit || reset) && (
        <div className="mt-1.5 flex flex-wrap justify-between gap-x-3 gap-y-0.5 text-[11px] text-muted-foreground">
          {usedLimit && <span className="tabular-nums">{usedLimit}</span>}
          {reset && <span>{reset}</span>}
        </div>
      )}
    </div>
  );
}

export function ServiceDetail({
  service,
  title,
}: {
  service: ServiceUsage;
  title: string;
}) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <>
      <DialogHeader>
        <div className="flex items-center justify-between gap-3 pr-8">
          <DialogTitle>{title}</DialogTitle>
          <Badge
            variant={service.connected ? "secondary" : "outline"}
            className={
              service.connected
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-500"
                : "text-muted-foreground"
            }
          >
            {service.connected ? <Wifi /> : <WifiOff />}
            {service.connected ? "Connected" : "Offline"}
          </Badge>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          {service.plan && (
            <Badge variant="outline" className="font-normal text-muted-foreground">
              {service.plan}
            </Badge>
          )}
          {service.account && (
            <span className="text-xs text-muted-foreground/80">{service.account}</span>
          )}
        </div>
      </DialogHeader>

      <div className="-mx-1 max-h-[55vh] space-y-2 overflow-y-auto px-1">
        {service.connected && service.windows.length > 0 ? (
          service.windows.map((w, i) => <DetailRow key={`${w.label}-${i}`} w={w} nowMs={nowMs} />)
        ) : (
          <p className="flex items-start gap-1.5 text-sm text-muted-foreground">
            <AlertCircle className="mt-0.5 shrink-0 text-muted-foreground/60" />
            <span className="break-words">
              {service.error?.trim() ??
                (service.connected ? "No usage windows reported." : "Not connected.")}
            </span>
          </p>
        )}
      </div>

      <DialogDescription className="pt-1 text-[11px]">
        {SOURCE_NOTE[service.provider]}
      </DialogDescription>
    </>
  );
}
