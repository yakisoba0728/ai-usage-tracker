import { describe, expect, it } from "vitest";

import {
  accountSubtitle,
  buildAccountSections,
  buildInspectorSummary,
  selectVisibleServiceId,
} from "@/lib/inspectorModel";
import type { AppConfig, Provider, ServiceUsage } from "@/lib/types";

const providers: Provider[] = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor",
  "zai",
];

const config: AppConfig = {
  schema_version: 1,
  poll_seconds: 300,
  providers: providers.map((_, index) => ({
    enabled: true,
    notify_thresholds: [70, 90],
    sort_index: index,
  })) as AppConfig["providers"],
  accounts: {},
  auto_anchor: {},
  launch_at_login: false,
  auto_update_check: true,
};

describe("inspector model", () => {
  it("groups visible accounts by online and offline state", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: true,
    });

    expect(sections.map((section) => section.key)).toEqual(["online", "offline"]);
    expect(sections.map((section) => section.count)).toEqual([4, 1]);
    expect(sections.flatMap((section) => section.rows.map((row) => row.id))).toEqual([
      "stored:claude-work",
      "auto:codex",
      "auto:zai",
      "auto:gemini",
      "auto:cursor",
    ]);
  });

  it("filters accounts by provider, label, plan, and account id", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "plus",
      showOffline: true,
    });

    expect(sections).toHaveLength(1);
    expect(sections[0]?.key).toBe("online");
    expect(sections[0]?.rows[0]?.id).toBe("auto:codex");
  });

  it("omits disabled providers from account sections", () => {
    const disabledCodex = {
      ...config,
      providers: config.providers.map((providerConfig, index) =>
        index === providers.indexOf("codex")
          ? { ...providerConfig, enabled: false }
          : providerConfig,
      ) as AppConfig["providers"],
    };

    const sections = buildAccountSections(sampleServices(), disabledCodex, {
      query: "",
      showOffline: true,
    });

    expect(sections.flatMap((section) => section.rows.map((row) => row.id))).not.toContain(
      "auto:codex",
    );
  });

  it("sorts account rows by the selected display preference", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: true,
      sortBy: "name",
    });

    const online = sections.find((section) => section.key === "online");
    expect(online?.rows.map((row) => row.id)).toEqual([
      "stored:claude-work",
      "auto:codex",
      "auto:gemini",
      "auto:zai",
    ]);
  });

  it("never renders a raw stored:<uuid> subtitle for a null-account stored service (BUG-6)", () => {
    const uuid = "9f8c1a2b-3d4e-5f60-7a8b-9c0d1e2f3a4b";
    const stored = service(`stored:${uuid}`, "claude", true, 12, null, "Max");

    const subtitle = accountSubtitle(stored, config);
    // The raw `stored:` prefix and the full uuid must never leak.
    expect(subtitle).not.toContain("stored:");
    expect(subtitle).not.toContain(uuid);
    // Provider label + a short id tail (last 6 of the stripped id).
    expect(subtitle).toContain("Claude");
    expect(subtitle).toContain(uuid.slice(-6));

    // The inspector summary id must not leak the raw stored:<uuid> either.
    const summary = buildInspectorSummary(stored, config);
    expect(summary.accountId).not.toContain("stored:");
    expect(summary.accountId).not.toContain(uuid);

    // And the card subtitle (via buildAccountSections) is the resolved string.
    const sections = buildAccountSections([stored], config, {
      query: "",
      showOffline: true,
    });
    expect(sections[0]?.rows[0]?.subtitle).toBe(subtitle);
  });

  it("prefers a real account label, then custom_name, over the synthesized fallback (BUG-6)", () => {
    const uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    const id = `stored:${uuid}`;
    // 1) real account label wins.
    const withAccount = service(id, "zai", true, 12, "z.ai workspace", "Pro");
    expect(accountSubtitle(withAccount, config)).toBe("z.ai workspace");
    // 2) null account but a per-account custom_name → custom_name.
    const namedConfig: AppConfig = {
      ...config,
      accounts: { [id]: { custom_name: "My zai key" } },
    };
    const noAccount = service(id, "zai", true, 12, null, "Pro");
    expect(accountSubtitle(noAccount, namedConfig)).toBe("My zai key");
  });

  it("keeps modal selection only while the selected account remains visible", () => {
    const sections = buildAccountSections(sampleServices(), config, {
      query: "",
      showOffline: false,
    });

    expect(selectVisibleServiceId("auto:codex", sections)).toBe("auto:codex");
    expect(selectVisibleServiceId("auto:cursor", sections)).toBeNull();
    expect(selectVisibleServiceId(null, [])).toBeNull();
  });
});

function sampleServices(): ServiceUsage[] {
  return [
    service("stored:claude-work", "claude", true, 98, "Claude - Work", "Max", [
      ["Messages", 98, 4_900, 5_000],
      ["Tokens (Input)", 64, 320_000, 500_000],
      ["Tokens (Output)", 84, 210_000, 250_000],
    ]),
    service("auto:codex", "codex", true, 82, "codex_9c2f7b", "Plus"),
    service("auto:gemini", "gemini", true, 36, "g_8bd91f22", "Code Assist"),
    service("auto:cursor", "cursor", false, null, null, null),
    service("auto:zai", "zai", true, 42, "z.ai workspace", "Pro"),
  ];
}

function service(
  id: string,
  provider: Provider,
  connected: boolean,
  pct: number | null,
  account: string | null,
  plan: string | null,
  detail: [string, number, number, number][] = [],
): ServiceUsage {
  return {
    id,
    source: id.startsWith("stored:") ? "stored" : "auto",
    provider,
    connected,
    plan,
    account,
    error: connected ? null : { code: "not_logged_in", detail: "Session not found." },
    windows:
      pct == null
        ? []
        : [
            {
              label: "Overall Usage",
              used_percent: pct,
              resets_at: 1_000_000 / 1000 + 15 * 60,
              used: 4_900,
              limit: 5_000,
            },
          ],
    detail_windows: detail.map(([label, usedPercent, used, limit]) => ({
      label,
      used_percent: usedPercent,
      resets_at: 1_000_000 / 1000 + 15 * 60,
      used,
      limit,
    })),
  };
}
