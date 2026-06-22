import { useEffect, useMemo, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Cloud, Database, ExternalLink, KeyRound, Loader2, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { authOptionsForProvider, type AddAccountOptionId } from "@/lib/addAccountOptions";
import {
  addSessionKey,
  cancelLogin,
  listAccounts,
  loginOAuth,
  onLoginComplete,
  removeAccount,
  startLogin,
} from "@/lib/ipc";
import { cn } from "@/lib/utils";
import type { AccountInfo, LoginInfo, Provider } from "@/lib/types";

const SUPPORTED: Provider[] = ["claude", "codex", "gemini", "copilot", "zai"];

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
  const [selectedProvider, setSelectedProvider] = useState<Provider | null>(null);
  const [info, setInfo] = useState<LoginInfo | null>(null);
  const [busy, setBusy] = useState<Provider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [sessionFor, setSessionFor] = useState<Provider | null>(null);
  const [sessionInput, setSessionInput] = useState("");

  const selectedOptions = useMemo(
    () => (selectedProvider ? authOptionsForProvider(selectedProvider) : []),
    [selectedProvider],
  );

  async function load() {
    try {
      setAccounts(await listAccounts());
    } catch {
      /* ignore - no backend in dev */
    }
  }

  useEffect(() => {
    if (open) {
      void load();
      return;
    }
    void cancelLogin().catch(() => {});
    setSelectedProvider(null);
    setInfo(null);
    setError(null);
    setBusy(null);
    setSessionFor(null);
    setSessionInput("");
  }, [open]);

  useEffect(() => {
    const unP = onLoginComplete((r) => {
      setBusy(null);
      if (r.ok) {
        setInfo(null);
        setError(null);
        void load();
        onChanged();
        onOpenChange(false);
      } else {
        setError(r.error ?? t("addAccount.loginFailed"));
      }
    });
    return () => {
      void unP.then((u) => u());
    };
  }, [onChanged, onOpenChange, t]);

  function chooseProvider(provider: Provider) {
    setSelectedProvider(provider);
    setError(null);
    setInfo(null);
    setSessionFor(null);
    setSessionInput("");
  }

  async function startBrowserOAuth(provider: Provider) {
    setBusy(provider);
    try {
      const url = await loginOAuth(provider);
      setInfo({ provider, verification_url: url, user_code: "", expires_in: 300 });
      await openUrl(url);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function startDeviceCode(provider: Provider) {
    setBusy(provider);
    try {
      const i = await startLogin(provider);
      setInfo(i);
      await openUrl(i.verification_url);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runOption(provider: Provider, optionId: AddAccountOptionId) {
    setError(null);
    setInfo(null);
    setSessionFor(null);
    setSessionInput("");

    if (optionId === "session-key") {
      setSessionFor(provider);
      return;
    }
    if (optionId === "browser-oauth") {
      await startBrowserOAuth(provider);
      return;
    }
    if (optionId === "device-code") {
      await startDeviceCode(provider);
      return;
    }

    onChanged();
    onOpenChange(false);
  }

  async function submitSession() {
    const provider = sessionFor;
    if (!provider || !sessionInput.trim()) return;
    setError(null);
    setBusy(provider);
    try {
      await addSessionKey(provider, sessionInput.trim());
      setSessionFor(null);
      setSessionInput("");
      await load();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(id: string) {
    try {
      await removeAccount(id);
    } catch (e) {
      setError(String(e));
      return;
    }
    await load();
    onChanged();
  }

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
          <section className="min-h-0 overflow-y-auto border-b border-border bg-[#1a1d20] p-3 sm:border-b-0 sm:border-r">
            <p className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
              {t("addAccount.provider")}
            </p>
            <div className="grid gap-1.5">
              {SUPPORTED.map((provider) => (
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

          <div className="scroll-area min-h-0 min-w-0 overflow-y-auto p-5">
            {selectedProvider ? (
              <div className="space-y-5">
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

                {sessionFor === selectedProvider && (
                  <Panel>
                    <div className="text-text" style={{ fontSize: 12 }}>
                      {t("addAccount.pasteKey", {
                        provider: PROVIDER_LABEL[sessionFor],
                      })}
                    </div>
                    <div className="mt-2 flex gap-2">
                      <input
                        className="num min-w-0 flex-1 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text placeholder:text-text-faint focus:border-border-strong"
                        style={{ fontSize: 12 }}
                        placeholder="sk-ant-... / token"
                        value={sessionInput}
                        autoFocus
                        onChange={(e) => setSessionInput(e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && void submitSession()}
                      />
                      <Button size="sm" onClick={() => void submitSession()} disabled={busy !== null || !sessionInput.trim()}>
                        {t("addAccount.add")}
                      </Button>
                    </div>
                    {t(`addAccount.hint.${sessionFor}`, { defaultValue: "" }) && (
                      <p className="mt-2 leading-relaxed text-text-faint" style={{ fontSize: 11 }}>
                        {t(`addAccount.hint.${sessionFor}`, { defaultValue: "" })}
                      </p>
                    )}
                  </Panel>
                )}

                {info && (
                  <Panel>
                    <button
                      type="button"
                      onClick={() => void openUrl(info.verification_url)}
                      className="num flex items-center gap-1 break-all text-text underline-offset-2 hover:underline"
                      style={{ fontSize: 12 }}
                    >
                      {info.verification_url}
                      <ExternalLink className="size-3 shrink-0" />
                    </button>
                    {info.user_code ? (
                      <div className="mt-2 text-text" style={{ fontSize: 12 }}>
                        {t("addAccount.enterCode")}{" "}
                        <span className="num select-all tracking-[0.3em]" style={{ fontSize: 15 }}>
                          {info.user_code}
                        </span>
                      </div>
                    ) : (
                      <div className="mt-2 text-text-dim" style={{ fontSize: 12 }}>
                        {t("addAccount.authorize")}
                      </div>
                    )}
                    <div className="mt-2 flex items-center gap-1.5 text-text-faint" style={{ fontSize: 11 }}>
                      <Loader2 className="size-3 animate-spin text-text-dim" />
                      {t("addAccount.waiting")}
                    </div>
                  </Panel>
                )}
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
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

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

function Panel({ children }: { children: ReactNode }) {
  return (
    <div className="space-y-1 rounded-lg border border-border bg-surface-2/60 p-3">
      {children}
    </div>
  );
}
