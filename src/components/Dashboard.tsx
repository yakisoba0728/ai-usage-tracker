import { Loader2 } from "lucide-react";

import { Header } from "@/components/Header";
import { ServiceCard } from "@/components/ServiceCard";
import { useUsage } from "@/hooks/useUsage";

export function Dashboard() {
  const { snapshot, loading, refresh } = useUsage();

  return (
    <div className="flex min-h-dvh flex-col gap-5 bg-background p-5 text-foreground">
      <Header
        fetchedAt={snapshot?.fetched_at ?? null}
        loading={loading}
        onRefresh={refresh}
      />

      {snapshot == null ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
          <Loader2 className="size-5 animate-spin" />
          <span>Loading usage…</span>
        </div>
      ) : snapshot.services.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-1 text-sm text-muted-foreground">
          <span>No services configured.</span>
          <span className="text-xs text-muted-foreground/70">
            Enable providers in settings to see usage.
          </span>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {snapshot.services.map((service) => (
            <ServiceCard
              key={service.provider}
              service={service}
            />
          ))}
        </div>
      )}
    </div>
  );
}
