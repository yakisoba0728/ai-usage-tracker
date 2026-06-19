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
  addSessionKey,
  cancelLogin,
  listAccounts,
  loginOAuth,
  onLoginComplete,
  removeAccount,
  startLogin,
} from "@/lib/ipc";
import type { LoginInfo, Provider, StoredCredential } from "@/lib/types";

/** Claude: paste a session key (from claude.ai cookies). */
const SESSION_KEY: Provider[] = ["claude", "copilot"];
/** Codex: browser + localhost-callback OAuth. */
const BROWSER_OAUTH: Provider[] = ["codex"];

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
  const [sessionFor, setSessionFor] = useState<Provider | null>(null);
  const [sessionInput, setSessionInput] = useState("");

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
    const unP = onLoginComplete((r) => {
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
    });
    return () => {
      void unP.then((u) => u());
    };
  }, [onChanged]);

  async function start(p: Provider) {
    setError(null);
    setBusy(p);
    setInfo(null);
    setSessionFor(null);
    setSessionInput("");
    try {
      if (SESSION_KEY.includes(p)) {
        // Show the session-key input (no OAuth/CLI).
        setSessionFor(p);
        setBusy(null);
      } else if (BROWSER_OAUTH.includes(p)) {
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
    await removeAccount(id);
    await load();
    onChanged();
  }

  function close() {
    cancelLogin();
    setInfo(null);
    setError(null);
    setBusy(null);
    setSessionFor(null);
    setSessionInput("");
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
              Claude: paste a session key. Codex: browser OAuth. Gemini/Copilot:
              device code. No password stored.
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
              Claude session key: claude.ai → DevTools → Application → Cookies →
              "sessionKey" value (starts with sk-ant-).
            </p>
          </section>

          {sessionFor && (
            <div className="space-y-1.5 rounded-md border bg-muted/30 p-3 text-xs">
              <div>Paste the {TITLES[sessionFor]} session key:</div>
              <div className="flex gap-2">
                <input
                  className="flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs"
                  placeholder="sk-ant-..."
                  value={sessionInput}
                  onChange={(e) => setSessionInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && submitSession()}
                />
                <Button size="sm" onClick={submitSession} disabled={!sessionInput.trim()}>
                  Add
                </Button>
              </div>
            </div>
          )}

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
                Waiting… (close X to cancel)
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

