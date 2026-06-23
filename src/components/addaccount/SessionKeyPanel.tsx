import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Panel } from "@/components/addaccount/Panel";
import { SESSION_INPUT_SECURITY_PROPS } from "@/lib/addAccountSecurity";
import { PROVIDER_LABEL } from "@/lib/providerMetadata";
import type { Provider } from "@/lib/types";

export interface SessionKeyPanelProps {
  sessionFor: Provider;
  sessionInput: string;
  busy: Provider | null;
  setSessionInput: (value: string) => void;
  submitSession: () => void;
}

export function SessionKeyPanel({
  sessionFor,
  sessionInput,
  busy,
  setSessionInput,
  submitSession,
}: SessionKeyPanelProps) {
  const { t } = useTranslation();
  return (
    <Panel>
      <div className="text-text" style={{ fontSize: 12 }}>
        {t("addAccount.pasteKey", {
          provider: PROVIDER_LABEL[sessionFor],
        })}
      </div>
      <div className="mt-2 flex gap-2">
        <input
          {...SESSION_INPUT_SECURITY_PROPS}
          className="num min-w-0 flex-1 rounded-md border border-border-strong bg-canvas px-2.5 py-1.5 text-text placeholder:text-text-faint focus:border-border-strong"
          style={{ fontSize: 12 }}
          placeholder="sk-ant-... / token"
          value={sessionInput}
          autoFocus
          aria-label={t("addAccount.sessionKeyInput", {
            provider: PROVIDER_LABEL[sessionFor],
          })}
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
  );
}
