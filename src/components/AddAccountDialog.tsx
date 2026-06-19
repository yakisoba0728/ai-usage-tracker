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
  listAccounts,
  onLoginComplete,
  removeAccount,
  startLogin,
} from "@/lib/ipc";
import type { LoginInfo, Provider, StoredCredential } from "@/lib/types";

const SUPPORTED: { p: Provider; name: string }[] = [
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

  // Listen for login completion for the lifetime of the component.
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
      const i = await startLogin(p);
      setInfo(i);
      await openUrl(i.verification_url);
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

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) {
          setInfo(null);
          setError(null);
        }
      }}
    >
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Plus className="size-4" />
          Add account
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Accounts</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 text-sm">
          <section>
            <p className="mb-2 text-xs text-muted-foreground">
              Add via OAuth — signs in as the official CLI (reuses its public
              client id):
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
              Claude/Cursor are auto-detect only (no public OAuth client).
            </p>
          </section>

          {info && (
            <div className="space-y-1.5 rounded-md border bg-muted/30 p-3 text-xs">
              <button
                className="inline-flex items-center gap-1 underline underline-offset-2"
                onClick={() => openUrl(info.verification_url)}
              >
                {info.verification_url}
                <ExternalLink className="size-3" />
              </button>
              <div>
                Enter code:{" "}
                <span className="font-mono text-base tracking-widest">
                  {info.user_code}
                </span>
              </div>
              <div className="flex items-center gap-1.5 text-muted-foreground">
                <Loader2 className="size-3 animate-spin" />
                Waiting for authorization…
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
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => remove(a.id)}
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

type UnlistenFn = () => void;
