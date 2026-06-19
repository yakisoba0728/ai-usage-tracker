import { useState, type ReactNode } from "react";
import { Trash2 } from "lucide-react";

import { BarGauge } from "@/components/BarGauge";
import { ProviderMark, PROVIDER_LABEL } from "@/components/ProviderMark";
import { removeAccount } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import {
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import type { Provider, ServiceUsage } from "@/lib/types";

/**
 * Source notes per provider — shown in the modal footer. Generic fallback so a
 * new provider needs no change here.
 */
const SOURCE_NOTE: Partial<Record<Provider, string>> = {
  claude: "Claude Code · api.anthropic.com",
  codex: "Codex CLI · chatgpt.com/backend-api",
  gemini: "Gemini CLI · Google Code Assist",
  copilot: "Copilot CLI · api.github.com/copilot_internal",
  cursor: "Cursor · api2.cursor.sh (experimental)",
  zai: "z.ai · glm coding plan",
};

export interface ProviderDetailProps {
  service: ServiceUsage;
  nowMs: number;
  /** Stored credential id, if this account was user-added (enables Remove). */
  accountId: string | null;
  onRemoved: () => void;
}

export function ProviderDetail({
  service,
  nowMs,
  accountId,
  onRemoved,
}: ProviderDetailProps) {
  const [tab, setTab] = useState<"usage" | "raw">("usage");
  const windows = service.windows ?? [];
  const detail = service.detail_windows ?? [];
  const hasRaw = Boolean(service.raw_response);
  const hasWindows = service.connected && windows.length > 0;
  const title = PROVIDER_LABEL[service.provider];
  const source = SOURCE_NOTE[service.provider] ?? `${title} usage`;

  async function handleRemove() {
    if (!accountId) return;
    try {
      await removeAccount(accountId);
    } catch (e) {
      console.error("remove_account failed:", e);
    } finally {
      onRemoved();
    }
  }

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 border-b border-border px-5 py-4 pr-12">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-surface-2 text-text-dim">
            <ProviderMark provider={service.provider} className="size-[18px]" />
          </span>
          <div className="min-w-0 leading-tight">
            <DialogTitle className="font-semibold tracking-tight text-text" style={{ fontSize: 16 }}>
              {title}
            </DialogTitle>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1">
              {service.plan && (
                <span className="rounded-md border border-border-strong bg-surface-2 px-1.5 py-0.5 font-medium text-text-dim" style={{ fontSize: 11 }}>
                  {service.plan}
                </span>
              )}
              <span
                className={cn("font-medium", service.connected ? "text-ok" : "text-text-faint")}
                style={{ fontSize: 11 }}
              >
                {service.connected ? "Connected" : "Offline"}
              </span>
              {service.account && (
                <span
                  className="num truncate text-text-faint"
                  style={{ fontSize: 11 }}
                  title={service.account}
                >
                  {service.account}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Tab bar — only when raw response is available */}
      {hasRaw && (
        <div className="flex gap-1 border-b border-border px-5 pt-2">
          {(["usage", "raw"] as const).map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={cn(
                "rounded-t-md border-b-2 px-3 py-2 transition-colors",
                tab === t
                  ? "border-signal text-text"
                  : "border-transparent text-text-faint hover:text-text-dim",
              )}
              style={{ fontSize: 12, fontWeight: 500 }}
            >
              {t === "usage" ? "Usage" : "Raw Response"}
            </button>
          ))}
        </div>
      )}

      {/* Body */}
      <div className="scroll-area max-h-[60vh] overflow-y-auto px-5 py-4">
        {tab === "raw" && hasRaw ? (
          <pre
            className="num overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-canvas p-3 text-text-dim"
            style={{ fontSize: 11, lineHeight: 1.6 }}
          >
            {service.raw_response}
          </pre>
        ) : hasWindows ? (
          <div className="space-y-5">
            <Section title="Usage">
              <div className="divide-y divide-border">
                {windows.map((w, i) => (
                  <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                    <BarGauge window={w} nowMs={nowMs} showMeta />
                  </div>
                ))}
              </div>
            </Section>

            {detail.length > 0 && (
              <Section title="More detail">
                <div className="divide-y divide-border">
                  {detail.map((w, i) => (
                    <div key={`${w.label}-${i}`} className="py-3 first:pt-0 last:pb-0">
                      <BarGauge window={w} nowMs={nowMs} showMeta />
                    </div>
                  ))}
                </div>
              </Section>
            )}
          </div>
        ) : (
          <div className="rounded-lg border border-border bg-surface-2/60 px-3.5 py-3 text-text-dim" style={{ fontSize: 13 }}>
            {service.error?.trim() ??
              (service.connected ? "No usage windows reported." : "Not connected.")}
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
        <DialogDescription className="text-text-faint" style={{ fontSize: 11 }}>
          {source}
        </DialogDescription>
        {accountId && (
          <button
            type="button"
            onClick={handleRemove}
            className="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-crit transition-colors hover:bg-crit/12"
            style={{ fontSize: 12, fontWeight: 500 }}
          >
            <Trash2 className="size-3.5" />
            Remove
          </button>
        )}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h4 className="mb-2 uppercase tracking-[0.06em] text-text-faint" style={{ fontSize: 10, fontWeight: 600 }}>
        {title}
      </h4>
      {children}
    </section>
  );
}
