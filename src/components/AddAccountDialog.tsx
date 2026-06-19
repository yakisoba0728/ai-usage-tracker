import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
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

/** Providers that add via pasting a raw key (no OAuth round-trip). */
const SESSION_KEY: Provider[] = ["claude", "copilot", "zai"];
/** Providers that add via browser + localhost-callback OAuth. */
const BROWSER_OAUTH: Provider[] = ["codex", "gemini"];
/**
 * Providers that ALSO support device-code OAuth as an alternative to pasting
 * a key. Shown as a "Sign in with X" link in the session-key panel.
 * - copilot: GitHub OAuth device-code (same flow `copilot login` uses, with
 *   the GitHub CLI's public client_id). Returns a `gho_…` token.
 */
const SESSION_KEY_WITH_OAUTH: Provider[] = ["copilot"];

/** Providers with an add flow (cursor is CLI-detected only — no add flow). */
const SUPPORTED: Provider[] = ["claude", "codex", "gemini", "copilot", "zai"];

/** Where to find the key, per session-key provider. */
const SESSION_HINT: Partial<Record<Provider, string>> = {
  claude: "claude.ai → DevTools → Application → Cookies → “sessionKey” (sk-ant-…).",
  copilot: "A GitHub token with Copilot access: gho_ (OAuth), ghu_ (GitHub App), or github_pat_ (fine-grained PAT with the “Copilot Requests” permission). Classic ghp_ tokens do NOT work.",
  zai: "Paste your z.ai GLM API key or session key.",
};

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
  const [info, setInfo] = useState<LoginInfo | null>(null);
  const [busy, setBusy] = useState<Provider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [sessionFor, setSessionFor] = useState<Provider | null>(null);
  const [sessionInput, setSessionInput] = useState("");

  async function load() {
    try {
      setAccounts(await listAccounts());
    } catch {
      /* ignore — no backend in dev */
    }
  }

  // Reset state + cancel any in-flight login whenever the dialog closes; load
  // accounts whenever it opens.
  useEffect(() => {
    if (open) {
      void load();
      return;
    }
    void cancelLogin().catch(() => {});
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
        setError(r.error ?? "Login failed.");
      }
    });
    return () => {
      void unP.then((u) => u());
    };
  }, [onChanged, onOpenChange]);

  // Device-code OAuth — used directly for non-session/non-browser providers
  // AND as a fallback for SESSION_KEY_WITH_OAUTH providers (e.g. Copilot).
  async function startDeviceCode(p: Provider) {
    setBusy(p);
    setSessionFor(null);
    try {
      const i = await startLogin(p);
      setInfo(i);
      await openUrl(i.verification_url);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function start(p: Provider) {
    setError(null);
    setInfo(null);
    setSessionFor(null);
    setSessionInput("");
    try {
      if (SESSION_KEY.includes(p)) {
        setBusy(null);
        setSessionFor(p);
      } else if (BROWSER_OAUTH.includes(p)) {
        // Codex (OpenAI PKCE) and Gemini (Google Authorization Code + loopback,
        // matching gemini-cli — Google's installed-app client_id does NOT
        // support the device-code grant).
        setBusy(p);
        const url = await loginOAuth(p);
        setInfo({ provider: p, verification_url: url, user_code: "", expires_in: 300 });
        await openUrl(url);
        setBusy(null);
      } else {
        // Device-code OAuth fallback.
        await startDeviceCode(p);
      }
    } catch (e) {
      setError(String(e));
      setBusy(null);
    }
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
      void load();
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
      <DialogContent className="gap-0 overflow-hidden rounded-lg border-border bg-surface p-0 sm:max-w-[440px]">
        <DialogHeader className="border-b border-border px-5 py-4 pr-12">
          <DialogTitle className="font-semibold tracking-tight text-text" style={{ fontSize: 16 }}>
            Add account
          </DialogTitle>
          <DialogDescription className="text-text-dim" style={{ fontSize: 12 }}>
            Session key, browser OAuth, or device code — no passwords stored.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-5 px-5 py-4">
          {/* Provider picker */}
          <section>
            <p className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
              Choose a provider
            </p>
            <div className="grid grid-cols-2 gap-2">
              {SUPPORTED.map((p) => (
                <button
                  key={p}
                  type="button"
                  disabled={busy !== null}
                  onClick={() => start(p)}
                  className={cn(
                    "flex items-center gap-2 rounded-lg border border-border bg-surface-2 px-2.5 py-2 text-left",
                    "transition-[background-color,border-color] duration-150",
                    "hover:border-border-strong hover:bg-surface-2",
                    "disabled:opacity-50",
                  )}
                >
                  <ProviderMark provider={p} className="size-4 shrink-0 text-text-dim" />
                  <span className="min-w-0 truncate text-text" style={{ fontSize: 12, fontWeight: 500 }}>
                    {PROVIDER_LABEL[p]}
                  </span>
                  {busy === p && <Loader2 className="ml-auto size-3.5 shrink-0 animate-spin text-text-dim" />}
                </button>
              ))}
            </div>
          </section>

          {/* Session-key paste */}
          {sessionFor && (
            <Panel>
              <div className="text-text" style={{ fontSize: 12 }}>
                Paste the {PROVIDER_LABEL[sessionFor]} key
              </div>
              <div className="mt-2 flex gap-2">
                <input
                  className="num min-w-0 flex-1 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text placeholder:text-text-faint focus:border-border-strong"
                  style={{ fontSize: 12 }}
                  placeholder="sk-ant-… / token"
                  value={sessionInput}
                  autoFocus
                  onChange={(e) => setSessionInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && submitSession()}
                />
                <Button size="sm" onClick={submitSession} disabled={busy !== null || !sessionInput.trim()}>
                  Add
                </Button>
              </div>
              {SESSION_HINT[sessionFor] && (
                <p className="mt-2 leading-relaxed text-text-faint" style={{ fontSize: 11 }}>
                  {SESSION_HINT[sessionFor]}
                </p>
              )}
              {SESSION_KEY_WITH_OAUTH.includes(sessionFor) && (
                <button
                  type="button"
                  disabled={busy !== null}
                  onClick={() => startDeviceCode(sessionFor)}
                  className="-mx-0.5 mt-2 cursor-pointer rounded-sm px-0.5 text-text underline decoration-text-faint underline-offset-2 transition-colors hover:decoration-text disabled:cursor-not-allowed disabled:opacity-50"
                  style={{ fontSize: 11, fontWeight: 500 }}
                >
                  Or sign in with {PROVIDER_LABEL[sessionFor]} instead →
                </button>
              )}
            </Panel>
          )}

          {/* Device-code / browser OAuth waiting state */}
          {info && (
            <Panel>
              <button
                type="button"
                onClick={() => openUrl(info.verification_url)}
                className="num flex items-center gap-1 break-all text-text underline-offset-2 hover:underline"
                style={{ fontSize: 12 }}
              >
                {info.verification_url}
                <ExternalLink className="size-3 shrink-0" />
              </button>
              {info.user_code ? (
                <div className="mt-2 text-text" style={{ fontSize: 12 }}>
                  Enter code:{" "}
                  <span className="num select-all tracking-[0.3em]" style={{ fontSize: 15 }}>
                    {info.user_code}
                  </span>
                </div>
              ) : (
                <div className="mt-2 text-text-dim" style={{ fontSize: 12 }}>
                  Authorize in your browser.
                </div>
              )}
              <div className="mt-2 flex items-center gap-1.5 text-text-faint" style={{ fontSize: 11 }}>
                <Loader2 className="size-3 animate-spin text-text-dim" />
                Waiting for authorization…
              </div>
            </Panel>
          )}

          {error && (
            <div className="rounded-md border border-border-strong bg-surface-2 px-3 py-2 text-text" style={{ fontSize: 12 }}>
              {error}
            </div>
          )}

          {/* Added accounts */}
          <section>
            <p className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
              Added accounts
            </p>
            {accounts.length === 0 ? (
              <p className="text-text-faint" style={{ fontSize: 12 }}>
                None yet.
              </p>
            ) : (
              <div className="space-y-1.5">
                {accounts.map((a) => (
                  <div
                    key={a.id}
                    className="flex items-center gap-2.5 rounded-md border border-border bg-surface-2 px-2.5 py-1.5"
                  >
                    <ProviderMark provider={a.provider} className="size-3.5 shrink-0 text-text-dim" />
                    <span className="min-w-0 flex-1 truncate text-text-dim" style={{ fontSize: 12 }}>
                      <span className="text-text">{PROVIDER_LABEL[a.provider]}</span>
                      {" — "}
                      {a.label}
                    </span>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => remove(a.id)}
                      aria-label={`Remove ${PROVIDER_LABEL[a.provider]} account`}
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
      </DialogContent>
    </Dialog>
  );
}

function Panel({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-1 rounded-lg border border-border bg-surface-2/60 p-3">
      {children}
    </div>
  );
}
