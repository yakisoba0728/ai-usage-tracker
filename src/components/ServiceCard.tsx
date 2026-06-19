import { AlertCircle, Wifi, WifiOff } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Gauge } from "@/components/Gauge";
import { ServiceDetail } from "@/components/ServiceDetail";
import type { Provider, ServiceUsage } from "@/lib/types";

const PROVIDER_TITLES: Record<Provider, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  copilot: "GitHub Copilot",
  cursor: "Cursor",
};

export interface ServiceCardProps {
  service: ServiceUsage;
}

export function ServiceCard({ service }: ServiceCardProps) {
  const title = PROVIDER_TITLES[service.provider] ?? service.provider;
  const hasWindows = service.connected && service.windows.length > 0;

  return (
    <Dialog>
      <DialogTrigger asChild>
        <Card className="cursor-pointer transition-colors hover:bg-muted/40 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring gap-4 py-5">
          <CardHeader className="gap-2 px-5">
            <div className="flex items-start justify-between gap-2">
              <CardTitle className="text-base">{title}</CardTitle>
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
                <span className="text-xs text-muted-foreground/80">
                  {service.account}
                </span>
              )}
            </div>
          </CardHeader>

          <CardContent className="px-5">
            {service.connected ? (
              hasWindows ? (
                <div className="space-y-3">
                  {service.windows.map((window, idx) => (
                    <Gauge
                      key={`${window.label}-${idx}`}
                      label={window.label}
                      used_percent={window.used_percent}
                      resets_at={window.resets_at}
                    />
                  ))}
                </div>
              ) : service.error ? (
                <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
                  <AlertCircle className="mt-0.5 shrink-0 text-muted-foreground/60" />
                  <span className="break-words">{service.error.trim()}</span>
                </p>
              ) : (
                <p className="text-xs text-muted-foreground">
                  No usage windows reported.
                </p>
              )
            ) : (
              <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
                <AlertCircle className="mt-0.5 shrink-0 text-muted-foreground/60" />
                <span className="break-words">
                  {service.error?.trim() || "Not connected."}
                </span>
              </p>
            )}
          </CardContent>
        </Card>
      </DialogTrigger>

      <DialogContent>
        <ServiceDetail service={service} title={title} />
      </DialogContent>
    </Dialog>
  );
}
