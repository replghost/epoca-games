import assert from "node:assert/strict";
import { stat } from "node:fs/promises";
import test from "node:test";

import {
  conformanceAppPaths,
  loadConformanceMatrix,
} from "../scripts/conformance.mjs";

test("the shared matrix covers every runtime profile and canonical app", async () => {
  const apps = await loadConformanceMatrix();
  assert.deepEqual(
    apps.map(({ id }) => id),
    [
      "doom",
      "quake",
      "duke3d",
      "nes",
      "egui-kitchen-sink",
      "gpu-cube",
      "scene-lab",
    ],
  );
  assert.deepEqual(
    new Set(apps.map(({ profile }) => profile)),
    new Set(["framebuffer", "tri2d", "webgpu-raster"]),
  );
  for (const app of apps) {
    const paths = conformanceAppPaths(app.id);
    assert.ok((await stat(paths.bundle)).isDirectory(), `${app.id} bundle`);
    assert.ok((await stat(paths.manifest)).isFile(), `${app.id} manifest`);
    assert.ok(app.inputKeys.length > 0, `${app.id} input contract`);
  }
});
