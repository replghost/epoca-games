import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

export const root = resolve(import.meta.dirname, "..");

export async function loadConformanceMatrix() {
  const source = JSON.parse(
    await readFile(resolve(root, "conformance", "matrix.json"), "utf8"),
  );
  if (source?.$v !== 1 || !Array.isArray(source.apps)) {
    throw new Error("invalid conformance matrix");
  }
  const ids = new Set();
  for (const app of source.apps) {
    if (
      typeof app?.id !== "string" ||
      !/^[a-z0-9][a-z0-9-]*$/.test(app.id) ||
      ids.has(app.id) ||
      !["framebuffer", "tri2d", "webgpu-raster"].includes(app.profile) ||
      !Array.isArray(app.inputKeys) ||
      app.inputKeys.some((key) => typeof key !== "string" || key.length === 0)
    ) {
      throw new Error(`invalid conformance entry ${JSON.stringify(app)}`);
    }
    ids.add(app.id);
  }
  return source.apps;
}

export function conformanceAppPaths(id) {
  const app = resolve(root, "apps", id);
  return {
    app,
    bundle: resolve(app, "bundle"),
    car: resolve(root, "dist", `${id}.car`),
    manifest: resolve(app, "bundle", "manifest.json"),
  };
}
