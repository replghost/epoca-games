import { spawn } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const allApps = [
  "doom",
  "quake",
  "duke3d",
  "nes",
  "egui-kitchen-sink",
  "gpu-cube",
  "scene-lab",
];
const selected = process.env.APP
  ? process.env.APP.split(",").map((value) => value.trim())
  : allApps;
for (const app of selected) {
  if (!allApps.includes(app)) throw new Error(`unknown app: ${app}`);
  await run(resolve(root, "apps", app, "package.sh"), [], root);
  await run(
    process.execPath,
    [resolve(root, "scripts", "prepare-app.mjs"), app],
    root,
  );
}

function run(command, args, cwd) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, {
      cwd,
      stdio: "inherit",
      env: process.env,
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolveRun();
      else reject(new Error(`${command} failed (${signal ?? code})`));
    });
  });
}
