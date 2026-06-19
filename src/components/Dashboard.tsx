import { Loader2 } from "lucide-react";

import { Header } from "@/components/Header";
import { ServiceCard } from "@/components/ServiceCard";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { useNow } from "@/hooks/useNow";
import { useUsage } from "@/hooks/useUsage";

export function Dashboard() {
  const { snapshot, loading, refresh } = useUsage();
  const nowMs = useNow(1000);

  // Only show connected/detected providers. Not-yet-detected ones stay hidden
  // and appear automatically once a token is found (local parsing) or an
  // account is added.
  const services = (snapshot?.services ?? []).filter((s) => s.connected);
  const connectedCount = services.length;

  // Highest used_percent across every window of every connected service
  // (primary + detail) — the honest "peak" surfaced in the header.
  let peak: number | null = null;
  for (const s of services) {
    if (!s.connected) continue;
    for (const w of [...(s.windows ?? []), ...(s.detail_windows ?? [])]) {
      if (w.used_percent != null && (peak == null || w.used_percent > peak)) {
        peak = w.used_percent;
      }
    }
  }

  return (
    <div className="relative min-h-dvh overflow-hidden bg-background text-foreground">
      <div aria-hidden className="ambient-glow pointer-events-none absolute inset-x-0 top-0 h-72" />

      <div className="relative flex min-h-dvh flex-col">
        <Header
          fetchedAt={snapshot?.fetched_at ?? null}
          loading={loading}
          onRefresh={refresh}
          nowMs={nowMs}
          peak={peak}
          connectedCount={connectedCount}
          totalCount={services.length}
        />

        <main className="mx-auto w-full max-w-6xl flex-1 px-5 py-6 sm:px-6 sm:py-8">
          <div className="mb-4 flex justify-end">
            <AddAccountDialog onChanged={refresh} />
          </div>
          {snapshot == null ? (
            <LoadingState />
          ) : services.length === 0 ? (
            <EmptyState />
          ) : (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {services.map((service, idx) => (
                <ServiceCard
                  key={`${service.provider}-${idx}`}
                  service={service}
                  nowMs={nowMs}
                />
              ))}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

function LoadingState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-sm text-muted-foreground">
      <Loader2 className="size-5 animate-spin text-brand" />
      <span>Loading usage…</span>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-1.5 py-24 text-center">
      <span className="text-sm font-medium text-foreground/90">
        No services connected
      </span>
      <span className="max-w-xs text-xs leading-relaxed text-muted-foreground/70">
        Sign in to a provider's CLI (e.g. <span className="font-mono">claude</span>,{" "}
        <span className="font-mono">codex login</span>), or click{" "}
        <span className="font-medium text-foreground/80">Add account</span> to sign
        in via OAuth.
      </span>
    </div>
  );
}
