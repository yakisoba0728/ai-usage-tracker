import { useEffect, useRef } from "react";
import { Plus, Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

export function AccountToolbar({
  query,
  onQueryChange,
  onAddAccount,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  onAddAccount: () => void;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="flex items-center gap-2 px-4 py-5">
      <label className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-text-faint" />
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          aria-label={t("toolbar.search")}
          placeholder={t("toolbar.searchPlaceholder")}
          className="h-10 w-full rounded-lg border border-border bg-surface/60 pl-9 pr-12 text-sm text-text placeholder:text-text-faint outline-none transition-colors focus:border-border-strong focus:bg-surface"
        />
        <span className="num absolute right-3 top-1/2 -translate-y-1/2 rounded border border-border bg-surface-2 px-1.5 py-0.5 text-[10px] text-text-faint">
          ⌘K
        </span>
      </label>

      <Button
        variant="outline"
        size="default"
        onClick={onAddAccount}
        aria-label={t("toolbar.addAccount")}
        className="h-10 gap-2 border-border bg-surface/80"
      >
        <Plus className="size-4" />
        <span className="hidden sm:inline">{t("toolbar.addAccount")}</span>
      </Button>
    </div>
  );
}
