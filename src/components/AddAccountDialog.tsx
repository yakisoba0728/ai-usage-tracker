import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  cancelLogin,
  listAccounts,
  loginOAuth,
  onLoginComplete,
  removeAccount,
  startLogin,
} from "@/lib/ipc";
import type { LoginInfo, Provider, StoredCredential } from "@/lib/types";

/** Claude/Codex use browser+localhost OAuth; Gemini/Copilot use device-code. */
const BROWSER_OAUTH: Provider[] = ["claude", "codex"];

const SUPPORTED: { p: Provider; name: string }[] = [
  { p: "claude", name: "Claude" },
  { p: "codex", name: "Codex" },
  { p: "gemini", name: "Gemini" },
  { p: "copilot", name: "GitHub Copilot" },
];

const TITLES: Record<Provider, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  copilot: "GitHub Copilot",
  cursor: "Cursor",
};

export function AddAccountDialog({ onChanged }: { onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  const [info, setInfo] = useState<LoginInfo | null>(null);
  const [busy, setBusy] = useState<Provider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<StoredCredential[]>([]);

  async function load() {
    try {
      setAccounts(await listAccounts());
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    if (open) load();
  }, [open]);

  useEffect(() => {
    let un: UnlistenFn | undefined;
    onLoginComplete((r) => {
      setBusy(null);
      if (r.ok) {
        setInfo(null);
        setError(null);
        void load();
        onChanged();
        setOpen(false);
      } else {
        setError(r.error ?? "login failed");
      }
    }).then((u) => {
      un = u;
    });
    return () => {
      un?.();
    };
  }, [onChanged]);

  async function start(p: Provider) {
    setError(null);
    setBusy(p);
    setInfo(null);
    try {
      if (BROWSER_OAUTH.includes(p)) {
        // Browser + localhost-callback flow (opens the provider's authorize URL).
        const url = await loginOAuth(p);
        setInfo({ provider: p, verification_url: url, user_code: "", expires_in: 300 });
        await openUrl(url);
      } else {
        const i = await startLogin(p);
        setInfo(i);
        await openUrl(i.verification_url);
      }
    } catch (e) {
      setError(String(e));
      setBusy(null);
    }
  }

  async function remove(id: string) {
    await removeAccount(id);
    await load();
    onChanged();
  }

  function close() {
    cancelLogin();
    setInfo(null);
    setError(null);
    setBusy(null);
    setOpen(false);
  }

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? setOpen(true) : close())}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Plus className="size-4" />
          Add account
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-md" onEscapeKeyDown={close} onPointerDownOutside={close}>
        <DialogHeader>
          <DialogTitle>Accounts</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 text-sm">
          <section>
            <p className="mb-2 text-xs text-muted-foreground">
              Sign in via OAuth — opens your browser; we capture the token on a
              local callback. No password stored.
            </p>
            <div className="flex flex-wrap gap-2">
              {SUPPORTED.map(({ p, name }) => (
                <Button
                  key={p}
                  variant="secondary"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => start(p)}
                >
                  {busy === p ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Plus className="size-4" />
                  )}
                  {name}
                </Button>
              ))}
            </div>
            <p className="mt-2 text-[11px] text-muted-foreground/70">
              Cursor is auto-detect only (no public OAuth client).
            </p>
          </section>

          {info && (
            <div className="space-y-1.5 rounded-md border bg-muted/30 p-3 text-xs">
              <button
                className="inline-flex items-center gap-1 break-all underline underline-offset-2"
                onClick={() => openUrl(info.verification_url)}
              >
                {info.verification_url}
                <ExternalLink className="size-3 shrink-0" />
              </button>
              {info.user_code ? (
                <div>
                  Enter code:{" "}
                  <span className="font-mono text-base tracking-widest">
                    {info.user_code}
                  </span>
                </div>
              ) : (
                <div>Authorize in your browser.</div>
              )}
              <div className="flex items-center gap-1.5 text-muted-foreground">
                <Loader2 className="size-3 animate-spin" />
                Waiting for authorization… (close X to cancel)
              </div>
            </div>
          )}

          {error && <div className="text-xs text-destructive">{error}</div>}

          <section>
            <p className="mb-2 text-xs text-muted-foreground">Added accounts:</p>
            {accounts.length === 0 ? (
              <p className="text-xs text-muted-foreground/60">None yet.</p>
            ) : (
              <div className="space-y-1">
                {accounts.map((a) => (
                  <div
                    key={a.id}
                    className="flex items-center justify-between rounded-md border px-2 py-1.5"
                  >
                    <span className="truncate">
                      {TITLES[a.provider]} — {a.label}
                    </span>
                    <Button variant="ghost" size="sm" onClick={() => remove(a.id)}>
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

type UnlistenFn = () => void;
