import { Cloud } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { AddedAccountsList } from "@/components/addaccount/AddedAccountsList";
import { AuthOptionList } from "@/components/addaccount/AuthOptionList";
import { DeviceCodePanel } from "@/components/addaccount/DeviceCodePanel";
import { ProviderRail } from "@/components/addaccount/ProviderRail";
import { SessionKeyPanel } from "@/components/addaccount/SessionKeyPanel";
import { useAddAccountFlow } from "@/hooks/useAddAccountFlow";
import { SESSION_INPUT_SECURITY_PROPS } from "@/lib/addAccountSecurity";

export { SESSION_INPUT_SECURITY_PROPS };

export interface AddAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged: () => void;
}

export function AddAccountDialog({
  open,
  onOpenChange,
  onChanged,
}: AddAccountDialogProps) {
  const { t } = useTranslation();
  const {
    selectedProvider,
    info,
    busy,
    sessionFor,
    sessionInput,
    error,
    accounts,
    chooseProvider,
    runOption,
    setSessionInput,
    submitSession,
    remove,
  } = useAddAccountFlow({ open, onOpenChange, onChanged, t });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid max-h-[88dvh] grid-rows-[auto_minmax(0,1fr)] gap-0 overflow-hidden rounded-lg border-border bg-surface p-0 sm:max-w-[760px]">
        <DialogHeader className="border-b border-border px-5 py-4 pr-12">
          <DialogTitle className="font-semibold tracking-tight text-text" style={{ fontSize: 16 }}>
            {t("addAccount.title")}
          </DialogTitle>
          <DialogDescription className="text-text-dim" style={{ fontSize: 12 }}>
            {t("addAccount.subtitle")}
          </DialogDescription>
        </DialogHeader>

        <div className="grid h-[min(560px,calc(88dvh-73px))] min-h-0 gap-0 sm:grid-cols-[235px_minmax(0,1fr)] max-sm:grid-rows-[auto_minmax(0,1fr)]">
          <ProviderRail
            selectedProvider={selectedProvider}
            busy={busy}
            chooseProvider={chooseProvider}
          />

          <div className="scroll-area min-h-0 min-w-0 overflow-y-auto p-5">
            {selectedProvider ? (
              <div className="space-y-5">
                <AuthOptionList
                  selectedProvider={selectedProvider}
                  busy={busy}
                  runOption={runOption}
                />

                {sessionFor === selectedProvider && (
                  <SessionKeyPanel
                    sessionFor={sessionFor}
                    sessionInput={sessionInput}
                    busy={busy}
                    setSessionInput={setSessionInput}
                    submitSession={submitSession}
                  />
                )}

                {info && <DeviceCodePanel info={info} />}
              </div>
            ) : (
              <div className="flex min-h-[260px] items-center justify-center rounded-lg border border-dashed border-border bg-surface-2/30 px-6 text-center">
                <div>
                  <Cloud className="mx-auto mb-3 size-6 text-text-faint" />
                  <p className="text-sm font-medium text-text">
                    {t("addAccount.select")}
                  </p>
                  <p className="mt-1 text-xs leading-5 text-text-faint">
                    {t("addAccount.selectHint")}
                  </p>
                </div>
              </div>
            )}

            {error && (
              <div className="mt-4 rounded-md border border-border-strong bg-surface-2 px-3 py-2 text-text" style={{ fontSize: 12 }}>
                {error}
              </div>
            )}

            <AddedAccountsList accounts={accounts} remove={remove} />
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
