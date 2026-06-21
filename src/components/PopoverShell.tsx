import { useEffect, useRef, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Plays a quick "drop down from the menu bar" entrance on the popover content.
 * The window itself just appears at its tray-anchored position; we animate the
 * webview content so it slides + fades into place. Replays every time the
 * popover is shown (each focus gain), since the window persists between shows.
 *
 * The translate is upward-to-zero and both this container and `body` are
 * `bg-canvas`, so the brief gap the slide exposes is the same color — seamless.
 */
function playDropIn(el: HTMLElement | null) {
  el?.animate(
    [
      { opacity: 0, transform: "translateY(-12px)" },
      { opacity: 1, transform: "translateY(0)" },
    ],
    { duration: 200, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
  );
}

export function PopoverShell({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    playDropIn(ref.current);
    let dispose: (() => void) | undefined;
    try {
      void getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => {
          if (focused) playDropIn(ref.current);
        })
        .then((un) => {
          dispose = un;
        })
        .catch(() => {});
    } catch {
      /* not in Tauri (browser preview) — the mount animation above is enough */
    }
    return () => dispose?.();
  }, []);

  return (
    <div ref={ref} className="h-dvh w-dvw overflow-hidden" style={{ willChange: "transform, opacity" }}>
      {children}
    </div>
  );
}
