import { spawn } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const dotli = process.env.DOTLI_REPO;
if (!dotli) throw new Error("DOTLI_REPO must point to a Dotli checkout");
const host = resolve(dotli, "apps", "host");
const apps = new Map([
  ["doom", "framebuffer"],
  ["quake", "framebuffer"],
  ["duke3d", "framebuffer"],
  ["nes", "framebuffer"],
  ["egui-kitchen-sink", "tri2d"],
  ["gpu-cube", "webgpu-raster"],
  ["scene-lab", "webgpu-raster"],
]);
for (const [app, profile] of apps) {
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
      DOTLI_DOOM_V2_CAR: resolve(root, "dist", `${app}.car`),
      DOTLI_DOOM_V2_MANIFEST: resolve(
        root,
        "apps",
        app,
        "bundle",
        "manifest.json",
      ),
      DOTLI_PVM_EXPECTED_BACKEND: "compiler",
      DOTLI_PVM_EXPECTED_PROFILE: profile,
      ...(profile === "webgpu-raster" ? { DOTLI_WEBGPU: "1" } : {}),
    },
  );
  console.log(`${app}: Dotli ${profile} smoke passed`);
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
