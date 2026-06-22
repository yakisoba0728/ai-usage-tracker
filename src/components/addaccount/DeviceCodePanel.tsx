import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Panel } from "@/components/addaccount/Panel";
import type { LoginInfo } from "@/lib/types";

export interface DeviceCodePanelProps {
  info: LoginInfo;
}

export function DeviceCodePanel({ info }: DeviceCodePanelProps) {
  const { t } = useTranslation();
  return (
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
  );
}
