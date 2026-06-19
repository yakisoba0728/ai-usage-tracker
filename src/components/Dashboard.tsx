import { useEffect, useMemo, useState } from "react";
import { Loader2 } from "lucide-react";

import { Header } from "@/components/Header";
import { FleetBar } from "@/components/FleetBar";
import { ProviderCard } from "@/components/ProviderCard";
import { ProviderDetail } from "@/components/ProviderDetail";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import {
  Dialog,
  DialogContent,
} from "@/components/ui/dialog";
import { useSnapshot } from "@/hooks/useSnapshot";
import { useNow } from "@/hooks/useNow";
import { listAccounts } from "@/lib/ipc";
import type { Provider, ServiceUsage } from "@/lib/types";

export function Dashboard() {
  const { snapshot, loading, refreshing, refresh } = useSnapshot();
  const nowMs = useNow(1000);

  const [addOpen, setAddOpen] = useState(false);
  const [openProvider, setOpenProvider] = useState<Provider | null>(null);
  const [openAccountId, setOpenAccountId] = useState<string | null>(null);

  // Connected providers get cards (connected-but-errored still shows a card).
  const services = useMemo<ServiceUsage[]>(
    () => (snapshot?.services ?? []).filter((s) => s.connected),
    [snapshot],
  );
  const totalCount = snapshot?.services?.length ?? 0;

  // Peak burn + which provider — across card-visible windows (not detail-only).
  const { peak, peakProvider } = useMemo(() => {
    let pk: number | null = null;
    let who: Provider | null = null;
    for (const s of services) {
      for (const w of s.windows ?? []) {
        if (w.used_percent != null && (pk == null || w.used_percent > pk)) {
          pk = w.used_percent;
          who = s.provider;
        }
      }
    }
    return { peak: pk, peakProvider: who };
  }, [services]);

  // When a detail modal opens, look up whether that provider has a stored
  // (user-added) credential — drives the Remove affordance.
  useEffect(() => {
    if (openProvider == null) {
      setOpenAccountId(null);
      return;
    }
    let cancelled = false;
    listAccounts()
      .then((list) => {
        if (cancelled) return;
        const match = list.find((a) => a.provider === openProvider);
        setOpenAccountId(match?.id ?? null);
      })
      .catch(() => {
        if (!cancelled) setOpenAccountId(null);
      });
    return () => {
      cancelled = true;
    };
  }, [openProvider]);

  const openService = useMemo<ServiceUsage | undefined>(
    () => snapshot?.services.find((s) => s.provider === openProvider),
    [snapshot, openProvider],
  );

  return (
    <div className="relative min-h-dvh overflow-hidden bg-canvas text-text">
      <div aria-hidden className="ambient pointer-events-none absolute inset-x-0 top-0 h-64" />

      <div className="relative flex min-h-dvh flex-col">
        <Header
          fetchedAt={snapshot?.fetched_at ?? null}
          refreshing={refreshing}
          onRefresh={refresh}
          onAddAccount={() => setAddOpen(true)}
          nowMs={nowMs}
          peak={peak}
          peakProvider={peakProvider}
          connectedCount={services.length}
          totalCount={totalCount}
        />

        <main className="mx-auto w-full max-w-[1100px] flex-1 px-5 py-6">
          {services.length > 0 && (
            <FleetBar services={services} onSelect={setOpenProvider} />
          )}

          {loading && snapshot == null ? (
            <LoadingState />
          ) : snapshot == null ? (
            <ErrorState error="Couldn't reach the tracker backend." />
          ) : services.length === 0 ? (
            <EmptyState onAddAccount={() => setAddOpen(true)} />
          ) : (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {services.map((service) => (
                <ProviderCard
                  key={service.provider}
                  service={service}
                  nowMs={nowMs}
                  onOpen={setOpenProvider}
                />
              ))}
            </div>
          )}
        </main>
      </div>

      {/* Detail modal — one instance, controlled by openProvider. */}
      <Dialog
        open={openProvider != null && openService != null}
        onOpenChange={(o) => !o && setOpenProvider(null)}
      >
        <DialogContent className="gap-0 overflow-hidden rounded-xl border-border bg-surface p-0 sm:max-w-lg">
          {openService && (
            <ProviderDetail
              service={openService}
              nowMs={nowMs}
              accountId={openAccountId}
              onRemoved={() => {
                void refresh();
                setOpenProvider(null);
              }}
            />
          )}
        </DialogContent>
      </Dialog>

      <AddAccountDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onChanged={refresh}
      />
    </div>
  );
}

function LoadingState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin text-signal" />
      <span style={{ fontSize: 13 }}>Loading usage…</span>
    </div>
  );
}
