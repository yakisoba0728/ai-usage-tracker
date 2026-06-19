import type { ReactNode } from "react";
import { AlertCircle } from "lucide-react";

import { BarGauge } from "@/components/Gauge";
import { StatusDot } from "@/components/StatusDot";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type { Provider, ServiceUsage } from "@/lib/types";

const SOURCE_NOTE: Record<Provider, string> = {
  claude: "Claude Code OAuth · api.anthropic.com",
  codex: "Codex CLI · chatgpt.com/backend-api/wham/usage",
  gemini: "Gemini CLI · Google Code Assist",
  copilot: "Copilot CLI · api.github.com/copilot_internal/user",
  cursor: "Cursor · api2.cursor.sh (experimental)",
};

export interface ServiceDetailProps {
  service: ServiceUsage;
  title: string;
  nowMs: number;
}

export function ServiceDetail({ service, title, nowMs }: ServiceDetailProps) {
  const windows = service.windows ?? [];
  const detail = service.detail_windows ?? [];
  const hasWindows = service.connected && windows.length > 0;

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 border-b border-white/[0.06] px-5 py-4 pr-12">
        <div className="flex min-w-0 items-center gap-2.5">
          <StatusDot connected={service.connected} className="mt-1" />
          <div className="min-w-0">
            <DialogTitle className="text-base font-semibold leading-tight tracking-tight">
              {title}
            </DialogTitle>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1">
              {service.plan && (
                <span className="rounded-md border border-white/10 bg-white/[0.03] px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground">
                  {service.plan}
                </span>
              )}
              <span
                className={cn(
                  "text-[11px] font-medium",
                  service.connected ? "text-ok" : "text-muted-foreground",
                )}
              >
                {service.connected ? "Connected" : "Offline"}
              </span>
              {service.account && (
                <span
                  className="truncate font-mono text-[11px] text-muted-foreground/70"
                  title={service.account}
                >
                  {service.account}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Body */}
      <div className="scroll-area max-h-[60vh] overflow-y-auto px-5 py-4">
        {hasWindows ? (
          <div className="space-y-5">
            <Section title="Usage">
              <div className="divide-y divide-white/[0.06]">
                {windows.map((w, i) => (
                  <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                    <BarGauge window={w} nowMs={nowMs} showMeta />
                  </div>
                ))}
              </div>
            </Section>

            {detail.length > 0 && (
              <Section title="More detail">
                <div className="divide-y divide-white/[0.06]">
                  {detail.map((w, i) => (
                    <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                      <BarGauge window={w} nowMs={nowMs} showMeta />
                    </div>
                  ))}
                </div>
              </Section>
            )}
          </div>
        ) : (
          <div className="flex items-start gap-2.5 rounded-xl border border-white/[0.05] bg-white/[0.015] px-3.5 py-3">
            <AlertCircle className="mt-0.5 size-4 shrink-0 text-muted-foreground/50" />
            <p className="break-words text-sm leading-relaxed text-muted-foreground/80">
              {service.error?.trim() ??
                (service.connected
                  ? "No usage windows reported."
                  : "Not connected.")}
            </p>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="border-t border-white/[0.06] px-5 py-3">
        <DialogDescription className="text-[11px] leading-relaxed text-muted-foreground/60">
          {SOURCE_NOTE[service.provider]}
        </DialogDescription>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/60">
        {title}
      </h3>
      {children}
    </section>
  );
}
