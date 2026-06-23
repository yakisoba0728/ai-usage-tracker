import type { SpawnOptions } from "node:child_process";

export interface TauriSpawnSpec {
  command: string;
  args: string[];
  options: SpawnOptions;
}

export function tauriSpawnSpec(
  platform: NodeJS.Platform | string,
  argv: string[],
  envSource: NodeJS.ProcessEnv | Record<string, string | undefined>,
): TauriSpawnSpec;
