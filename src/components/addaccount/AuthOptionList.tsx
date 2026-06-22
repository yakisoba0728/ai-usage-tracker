import { useMemo } from "react";
import { Cloud, Database, KeyRound } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ProviderMark } from "@/components/ProviderMark";
import {
  authOptionsForProvider,
  type AddAccountOptionId,
} from "@/lib/addAccountOptions";
import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import type { Provider } from "@/lib/types";

function optionIcon(optionId: AddAccountOptionId) {
  switch (optionId) {
    case "session-key":
      return <KeyRound className="size-4" />;
    case "browser-oauth":
      return <Cloud className="size-4" />;
    case "device-code":
      return <KeyRound className="size-4" />;
    case "local-session":
      return <Database className="size-4" />;
  }
}

export interface AuthOptionListProps {
  selectedProvider: Provider;
  busy: Provider | null;
  runOption: (provider: Provider, optionId: AddAccountOptionId) => void;
}

export function AuthOptionList({
  selectedProvider,
  busy,
  runOption,
}: AuthOptionListProps) {
  const { t } = useTranslation();
  const selectedOptions = useMemo(
    () => authOptionsForProvider(selectedProvider),
    [selectedProvider],
  );

  return (
    <section>
      <div className="flex items-start gap-3">
        <ProviderMark provider={selectedProvider} className="mt-0.5 size-6 shrink-0 text-text-dim" />
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-text">
            {PROVIDER_LABEL[selectedProvider]}
          </h3>
          <p className="mt-1 text-xs leading-5 text-text-faint">
            {t(`addAccount.copy.${selectedProvider}`)}
          </p>
        </div>
      </div>

      <div className="mt-4 grid gap-2">
        {selectedOptions.map((optionId) => (
          <button
            key={optionId}
            type="button"
            disabled={busy !== null}
            onClick={() => void runOption(selectedProvider, optionId)}
            className="flex w-full items-start gap-3 rounded-lg border border-border bg-surface-2/60 px-3 py-3 text-left transition-colors hover:border-border-strong hover:bg-surface-2 disabled:opacity-50"
          >
            <span className="mt-0.5 text-text-dim">{optionIcon(optionId)}</span>
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-text">
                {t(`addAccount.option.${optionId}.title`)}
              </span>
              <span className="mt-0.5 block text-xs leading-4 text-text-faint">
                {t(`addAccount.option.${optionId}.description`)}
              </span>
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
