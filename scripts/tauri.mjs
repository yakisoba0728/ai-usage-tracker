import { spawn } from "node:child_process";

import { tauriSpawnSpec } from "./tauriCommand.mjs";

const spec = tauriSpawnSpec(process.platform, process.argv.slice(2), process.env);
const child = spawn(spec.command, spec.args, spec.options);

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});

child.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});
