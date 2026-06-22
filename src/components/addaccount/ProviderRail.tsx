import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ProviderMark } from "@/components/ProviderMark";
import { ADD_ACCOUNT_PROVIDERS } from "@/lib/addAccountOptions";
import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import { cn } from "@/lib/utils";
import type { Provider } from "@/lib/types";

export interface ProviderRailProps {
  selectedProvider: Provider | null;
  busy: Provider | null;
  chooseProvider: (provider: Provider) => void;
}

export function ProviderRail({
  selectedProvider,
  busy,
  chooseProvider,
}: ProviderRailProps) {
  const { t } = useTranslation();
  return (
    <section className="min-h-0 overflow-y-auto border-b border-border bg-[#1a1d20] p-3 sm:border-b-0 sm:border-r">
      <p className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
        {t("addAccount.provider")}
      </p>
      <div className="grid gap-1.5">
        {ADD_ACCOUNT_PROVIDERS.map((provider) => (
          <button
            key={provider}
            type="button"
            disabled={busy !== null}
            onClick={() => chooseProvider(provider)}
            className={cn(
              "flex items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors disabled:opacity-50",
              selectedProvider === provider
                ? "border-border-strong bg-surface text-text"
                : "border-transparent bg-transparent text-text-dim hover:bg-surface/70 hover:text-text",
            )}
          >
            <ProviderMark provider={provider} className="size-4 shrink-0" />
            <span className="min-w-0 flex-1 truncate text-xs font-medium">
              {PROVIDER_LABEL[provider]}
            </span>
            {busy === provider && <Loader2 className="size-3.5 shrink-0 animate-spin text-text-dim" />}
          </button>
        ))}
      </div>
    </section>
  );
}
