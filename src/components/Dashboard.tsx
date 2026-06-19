import { useEffect, useMemo, useState } from "react";
import { Loader2 } from "lucide-react";

import { Header } from "@/components/Header";
import { ProviderCard } from "@/components/ProviderCard";
import { ProviderDetail } from "@/components/ProviderDetail";
import { AddAccountDialog } from "@/components/AddAccountDialog";
import { SettingsDialog, type SortBy } from "@/components/SettingsDialog";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { useSnapshot } from "@/hooks/useSnapshot";
import { useNow } from "@/hooks/useNow";
import { listAccounts } from "@/lib/ipc";
import { PROVIDER_LABEL } from "@/components/ProviderMark";
import type { Provider, ServiceUsage } from "@/lib/types";

export function Dashboard() {
  const { snapshot, loading, refreshing, refresh } = useSnapshot();
  const nowMs = useNow(1000);

  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openProvider, setOpenProvider] = useState<Provider | null>(null);
  const [openAccountId, setOpenAccountId] = useState<string | null>(null);

  // Display preferences (local — not persisted to the backend).
  const [sortBy, setSortBy] = useState<SortBy>("usage");
  const [showOffline, setShowOffline] = useState(false);

  const allServices = snapshot?.services ?? [];

  // Show connected cards (plus offline ones when "Show offline" is on), in the
  // chosen order: by primary-window usage desc, or alphabetical by label.
  const services = useMemo<ServiceUsage[]>(() => {
    const list = (
      showOffline ? allServices : allServices.filter((s) => s.connected)
    ).slice();
    list.sort((a, b) => {
      if (sortBy === "name") {
        return PROVIDER_LABEL[a.provider].localeCompare(
          PROVIDER_LABEL[b.provider],
        );
      }
      const pa = a.windows?.[0]?.used_percent ?? -1;
      const pb = b.windows?.[0]?.used_percent ?? -1;
      return pb - pa;
    });
    return list;
  }, [allServices, showOffline, sortBy]);

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

  const hasConfigured = allServices.length > 0;

  return (
    <div className="relative flex min-h-dvh flex-col bg-canvas text-text">
      <Header
        refreshing={refreshing}
        onRefresh={refresh}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <main className="mx-auto w-full max-w-[1100px] flex-1 px-5 py-6">
        {loading && snapshot == null ? (
          <LoadingState />
        ) : snapshot == null ? (
          <ErrorState error="Couldn't reach the tracker backend." />
        ) : !hasConfigured ? (
          <EmptyState onAddAccount={() => setAddOpen(true)} />
        ) : services.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 py-24 text-center">
            <p className="text-text-dim" style={{ fontSize: 13 }}>
              No connected providers right now.
            </p>
            <p className="text-text-faint" style={{ fontSize: 12 }}>
              Enable “Show offline” in Settings to see all configured providers.
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
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

      {/* Detail modal — one instance, controlled by openProvider. */}
      <Dialog
        open={openProvider != null && openService != null}
        onOpenChange={(o) => !o && setOpenProvider(null)}
      >
        <DialogContent className="gap-0 overflow-hidden rounded-lg border-border bg-surface p-0 sm:max-w-lg">
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

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        sortBy={sortBy}
        onSortByChange={setSortBy}
        showOffline={showOffline}
        onShowOfflineChange={setShowOffline}
        onAddAccount={() => {
          setSettingsOpen(false);
          setAddOpen(true);
        }}
      />
    </div>
  );
}

function LoadingState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 py-24 text-text-dim">
      <Loader2 className="size-5 animate-spin" />
      <span style={{ fontSize: 13 }}>Loading usage…</span>
    </div>
  );
}
