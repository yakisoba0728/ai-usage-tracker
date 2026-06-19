import { AlertCircle, ChevronRight } from "lucide-react";

import { BarGauge } from "@/components/Gauge";
import { RingGauge } from "@/components/RingGauge";
import { ServiceDetail } from "@/components/ServiceDetail";
import { StatusDot } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogTrigger,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { formatResetCountdown, formatUsedLimit } from "@/lib/format";
import type { LimitWindow, Provider, ServiceUsage } from "@/lib/types";

const PROVIDER_TITLES: Record<Provider, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  copilot: "GitHub Copilot",
  cursor: "Cursor",
};

/** Pick the most pressing primary window as the headline (highest used_percent). */
function pickHeadline(windows: LimitWindow[]): LimitWindow | null {
  const withPct = windows.filter((w) => w.used_percent != null);
  if (withPct.length > 0) {
    return withPct.reduce((a, b) =>
      (b.used_percent ?? 0) > (a.used_percent ?? 0) ? b : a,
    );
  }
  return windows[0] ?? null;
}

export interface ServiceCardProps {
  service: ServiceUsage;
  nowMs: number;
}

export function ServiceCard({ service, nowMs }: ServiceCardProps) {
  const title = PROVIDER_TITLES[service.provider] ?? service.provider;
  const windows = service.windows ?? [];
  const headline = service.connected ? pickHeadline(windows) : null;
  const secondary = headline ? windows.filter((w) => w !== headline) : [];
  const headlineReset = headline
    ? formatResetCountdown(headline.resets_at, nowMs)
    : null;
  const headlineUsedLimit = headline ? formatUsedLimit(headline) : null;

  return (
    <Dialog>
      <DialogTrigger asChild>
        <Card
          className={cn(
            "group relative w-full cursor-pointer gap-0 overflow-hidden rounded-2xl border-white/[0.06] bg-card px-5 py-5",
            "shadow-[inset_0_1px_0_0_rgba(255,255,255,0.03),0_12px_30px_-18px_rgba(0,0,0,0.75)]",
            "transition-all duration-200 ease-out",
            "hover:-translate-y-0.5 hover:border-white/[0.12] hover:bg-card/80",
            "hover:shadow-[inset_0_1px_0_0_rgba(255,255,255,0.05),0_22px_42px_-20px_rgba(0,0,0,0.85)]",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-ring/60",
          )}
        >
          {/* Header */}
          <div className="flex items-start justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2.5">
              <StatusDot connected={service.connected} className="mt-1" />
              <div className="min-w-0">
                <h3 className="truncate text-[15px] font-semibold leading-tight tracking-tight">
                  {title}
                </h3>
                {service.account && (
                  <p
                    className="truncate text-xs text-muted-foreground/80"
                    title={service.account}
                  >
                    {service.account}
                  </p>
                )}
              </div>
            </div>
            {service.plan && (
              <Badge
                variant="outline"
                className="shrink-0 border-white/10 bg-white/[0.03] font-medium text-muted-foreground"
              >
                {service.plan}
              </Badge>
            )}
          </div>

          {/* Body */}
          <div className="mt-4">
            {service.connected && headline ? (
              <>
                <div className="flex items-center gap-4">
                  <RingGauge value={headline.used_percent} />
                  <div className="min-w-0 flex-1">
                    <div className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                      {headline.label}
                    </div>
                    {headlineReset && (
                      <div className="mt-1.5 font-mono text-xs tabular-nums text-muted-foreground">
                        {headlineReset}
                      </div>
                    )}
                    {headlineUsedLimit && (
                      <div className="mt-0.5 font-mono text-[11px] tabular-nums text-muted-foreground/70">
                        {headlineUsedLimit}
                      </div>
                    )}
                  </div>
                </div>

                {secondary.length > 0 && (
                  <>
                    <div className="my-4 h-px bg-white/[0.06]" />
                    <div className="space-y-3">
                      {secondary.map((w, i) => (
                        <BarGauge
                          key={`${w.label}-${i}`}
                          window={w}
                          nowMs={nowMs}
                          showMeta={false}
                        />
                      ))}
                    </div>
                  </>
                )}
              </>
            ) : (
              <EmptyState
                text={
                  service.connected
                    ? service.error?.trim() || "No usage windows reported."
                    : service.error?.trim() || "Not connected."
                }
              />
            )}
          </div>

          {/* Footer affordance — signals clickability */}
          <div className="mt-4 flex items-center justify-between border-t border-white/[0.06] pt-3 text-[11px] font-medium text-muted-foreground/60 transition-colors group-hover:text-muted-foreground">
            <span>View details</span>
            <ChevronRight className="size-3.5 transition-transform duration-200 group-hover:translate-x-0.5" />
          </div>
        </Card>
      </DialogTrigger>

      <DialogContent className="gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-lg sm:rounded-2xl">
        <ServiceDetail service={service} title={title} nowMs={nowMs} />
      </DialogContent>
    </Dialog>
  );
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="flex min-h-[96px] items-center gap-2.5 rounded-xl border border-white/[0.05] bg-white/[0.015] px-3.5 py-3">
      <AlertCircle className="mt-0.5 size-4 shrink-0 text-muted-foreground/50" />
      <p className="break-words text-xs leading-relaxed text-muted-foreground/80">
        {text}
      </p>
    </div>
  );
}
