export function tauriSpawnSpec(platform, argv, envSource) {
  const env = { ...envSource };
  delete env.CI;

  const isWindows = platform === "win32";
  return {
    command: isWindows ? "pnpm.cmd" : "pnpm",
    args: ["exec", "tauri", ...argv],
    options: {
      stdio: "inherit",
      env,
      shell: isWindows,
    },
  };
}
