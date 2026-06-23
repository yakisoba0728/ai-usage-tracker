import { describe, expect, it } from "vitest";

import { tauriSpawnSpec } from "../../scripts/tauriCommand.mjs";

describe("tauri command wrapper", () => {
  it("runs the Windows pnpm shim through a shell", () => {
    const spec = tauriSpawnSpec("win32", ["build", "--debug"], {
      CI: "true",
      PATH: "C:\\tools",
    });

    expect(spec.command).toBe("pnpm.cmd");
    expect(spec.args).toEqual(["exec", "tauri", "build", "--debug"]);
    expect(spec.options.shell).toBe(true);
    expect(spec.options.env).toEqual({ PATH: "C:\\tools" });
  });

  it("keeps non-Windows pnpm execution shell-free", () => {
    const spec = tauriSpawnSpec("darwin", ["build"], {
      CI: "true",
      PATH: "/usr/bin",
    });

    expect(spec.command).toBe("pnpm");
    expect(spec.args).toEqual(["exec", "tauri", "build"]);
    expect(spec.options.shell).toBe(false);
    expect(spec.options.env).toEqual({ PATH: "/usr/bin" });
  });
});
