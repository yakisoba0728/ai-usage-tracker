import { useTranslation } from "react-i18next";

import type { ServiceUsage } from "@/lib/types";

export function RawTab({ service }: { service: ServiceUsage }) {
  const { t } = useTranslation();
  if (!service.raw_response) {
    return (
      <div className="rounded-lg border border-border bg-surface/50 p-5 text-sm text-text-faint">
        {t("detail.raw.empty")}
      </div>
    );
  }
  return (
    <pre className="scroll-area max-h-[460px] overflow-auto rounded-lg border border-border bg-canvas p-4 text-xs leading-relaxed text-text-dim">
      <code className="num">{service.raw_response}</code>
    </pre>
  );
}
