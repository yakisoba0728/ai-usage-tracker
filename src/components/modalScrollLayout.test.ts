import { describe, expect, it } from "vitest";

import addAccount from "./AddAccountDialog.tsx?raw";
import dashboard from "./Dashboard.tsx?raw";
import settings from "./SettingsDialog.tsx?raw";

describe("modal scroll layout", () => {
  it("constrains add-account modal body so the inner pane can scroll", () => {
    expect(addAccount).toContain("grid-rows-[auto_minmax(0,1fr)]");
    expect(addAccount).toContain("grid h-[min(560px,calc(88dvh-73px))] min-h-0");
    expect(addAccount).toContain("scroll-area min-h-0 min-w-0 overflow-y-auto");
  });

  it("keeps settings modal panes shrinkable on desktop and mobile", () => {
    expect(settings).toContain("max-md:grid-rows-[auto_minmax(0,1fr)]");
    expect(settings).toContain("scroll-area min-h-0 min-w-0 overflow-y-auto");
  });

  it("sizes detail modal content instead of clipping a full-viewport child", () => {
    expect(dashboard).toContain("h-[min(760px,86dvh)]");
    expect(dashboard).toContain("scroll-area h-full min-h-0 overflow-y-auto");
    expect(dashboard).not.toContain("lg:h-dvh");
  });
});
