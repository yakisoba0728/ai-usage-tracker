import { Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ProviderMark } from "@/components/ProviderMark";
import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import type { AccountInfo } from "@/lib/types";

export interface AddedAccountsListProps {
  accounts: AccountInfo[];
  remove: (id: string) => void;
}

export function AddedAccountsList({ accounts, remove }: AddedAccountsListProps) {
  const { t } = useTranslation();
  return (
    <section className="mt-5">
      <p className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
        {t("addAccount.added")}
      </p>
      {accounts.length === 0 ? (
        <p className="text-text-faint" style={{ fontSize: 12 }}>
          {t("addAccount.none")}
        </p>
      ) : (
        <div className="space-y-1.5">
          {accounts.map((account) => (
            <div
              key={account.id}
              className="flex items-center gap-2.5 rounded-md border border-border bg-surface-2 px-2.5 py-1.5"
            >
              <ProviderMark provider={account.provider} className="size-3.5 shrink-0 text-text-dim" />
              <span className="min-w-0 flex-1 truncate text-text-dim" style={{ fontSize: 12 }}>
                <span className="text-text">{PROVIDER_LABEL[account.provider]}</span>
                {" - "}
                {account.label}
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => void remove(account.id)}
                aria-label={t("addAccount.removeAria", {
                  provider: PROVIDER_LABEL[account.provider],
                })}
                className="text-text-faint hover:text-text"
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
