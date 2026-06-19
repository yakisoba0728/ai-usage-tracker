import type { Provider } from "@/lib/types";

/**
 * Monochrome inline SVG provider marks — designed to read at 16px. All use
 * `currentColor` so severity/brand coloring applies. Geometric, not literal
 * logos: each is a recognizable abstraction of the provider's visual language.
 */

export function ClaudeMark({ className }: { className?: string }) {
  // Anthropic sunburst — a central node with eight radiating rays.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <g stroke="currentColor" strokeWidth="1.7" strokeLinecap="round">
        <line x1="12" y1="3" x2="12" y2="6.5" />
        <line x1="12" y1="17.5" x2="12" y2="21" />
        <line x1="3" y1="12" x2="6.5" y2="12" />
        <line x1="17.5" y1="12" x2="21" y2="12" />
        <line x1="5.6" y1="5.6" x2="8.1" y2="8.1" />
        <line x1="15.9" y1="15.9" x2="18.4" y2="18.4" />
        <line x1="18.4" y1="5.6" x2="15.9" y2="8.1" />
        <line x1="8.1" y1="15.9" x2="5.6" y2="18.4" />
      </g>
      <circle cx="12" cy="12" r="2.4" fill="currentColor" />
    </svg>
  );
}

export function CodexMark({ className }: { className?: string }) {
  // OpenAI-style hex node — a hexagon with an inner core.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <path
        d="M12 2.5 20 7v10l-8 4.5L4 17V7z"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinejoin="round"
      />
      <path
        d="M12 7.2 16 9.5v5L12 16.8 8 14.5v-5z"
        fill="currentColor"
        opacity="0.9"
      />
    </svg>
  );
}

export function GeminiMark({ className }: { className?: string }) {
  // Google Gemini four-point sparkle.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <path
        d="M12 2c.5 4.6 2.4 6.5 7 7-4.6.5-6.5 2.4-7 7-.5-4.6-2.4-6.5-7-7 4.6-.5 6.5-2.4 7-7z"
        fill="currentColor"
      />
    </svg>
  );
}

export function CopilotMark({ className }: { className?: string }) {
  // GitHub-style hub — a center node linked to three orbital satellites.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <g stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
        <line x1="12" y1="12" x2="12" y2="5" />
        <line x1="12" y1="12" x2="18" y2="16" />
        <line x1="12" y1="12" x2="6" y2="16" />
      </g>
      <circle cx="12" cy="5" r="2.1" fill="currentColor" />
      <circle cx="18" cy="16" r="2.1" fill="currentColor" />
      <circle cx="6" cy="16" r="2.1" fill="currentColor" />
      <circle cx="12" cy="12" r="2" fill="currentColor" />
    </svg>
  );
}

export function CursorMark({ className }: { className?: string }) {
  // A mouse cursor / arrow.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <path
        d="M5 3.5 19 11l-5.8 1.4L16 18l-2.6 1L10 13.6 6.5 18 5 3.5z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="0.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function ZaiMark({ className }: { className?: string }) {
  // z.ai — a four-line spark/lightning.
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <path
        d="M13 2.5 5 13.2h5.4L9.6 21.5 18 10.4h-5.2L13 2.5z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="0.4"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const MARKS: Record<Provider, (p: { className?: string }) => React.ReactElement> =
  {
    claude: ClaudeMark,
    codex: CodexMark,
    gemini: GeminiMark,
    copilot: CopilotMark,
    cursor: CursorMark,
    zai: ZaiMark,
  };

/** Resolve a provider to its mark — the one provider-agnostic seam. */
export function ProviderMark({
  provider,
  className,
}: {
  provider: Provider;
  className?: string;
}) {
  const M = MARKS[provider] ?? CodexMark;
  return <M className={className} />;
}

/** Human label per provider — used in headers, modal titles, empty rows. */
export const PROVIDER_LABEL: Record<Provider, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  copilot: "GitHub Copilot",
  cursor: "Cursor",
  zai: "z.ai",
};

/**
 * Brand mark — a ring with a moving arc (echoes the signature gauge).
 * `pct` (0–100) drives the arc length; defaults to a calm telemetry sweep.
 */
export function BrandMark({
  className,
  pct = 68,
}: {
  className?: string;
  pct?: number;
}) {
  const r = 9;
  const c = 2 * Math.PI * r;
  const clamped = Math.max(0, Math.min(100, pct));
  const offset = c * (1 - clamped / 100);
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" aria-hidden>
      <circle
        cx="12"
        cy="12"
        r={r}
        stroke="currentColor"
        strokeWidth="2"
        opacity="0.22"
      />
      <circle
        cx="12"
        cy="12"
        r={r}
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={offset}
        transform="rotate(-90 12 12)"
      />
      <circle cx="12" cy="12" r="2.3" fill="currentColor" />
    </svg>
  );
}
