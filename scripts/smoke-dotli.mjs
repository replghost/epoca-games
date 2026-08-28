import { spawn } from "node:child_process";
import { resolve } from "node:path";

import {
  conformanceAppPaths,
  loadConformanceMatrix,
} from "./conformance.mjs";
const dotli = process.env.DOTLI_REPO;
if (!dotli) throw new Error("DOTLI_REPO must point to a Dotli checkout");
const host = resolve(dotli, "apps", "host");
const apps = await loadConformanceMatrix();
for (const app of apps) {
  const paths = conformanceAppPaths(app.id);
  await run(
    "bunx",
    [
      "playwright",
      "test",
      "--config=tests/functional/playwright.config.ts",
      "tests/functional/pvm.spec.ts",
      "-g",
      "canonical Doom",
    ],
    host,
    {
      ...process.env,
      DOTLI_DOOM_V2_CAR: paths.car,
      DOTLI_DOOM_V2_MANIFEST: paths.manifest,
      DOTLI_PVM_EXPECTED_BACKEND: "compiler",
      DOTLI_PVM_EXPECTED_PROFILE: app.profile,
      DOTLI_PVM_INPUT_KEYS: app.inputKeys.join(","),
      ...(app.profile === "webgpu-raster" ? { DOTLI_WEBGPU: "1" } : {}),
    },
  );
  console.log(`${app.id}: Dotli ${app.profile} smoke passed`);
}

function run(command, args, cwd, env) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, { cwd, env, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolveRun();
      else reject(new Error(`${command} failed (${signal ?? code})`));
    });
  });
}
