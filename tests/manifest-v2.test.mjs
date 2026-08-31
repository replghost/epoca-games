import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const expected = {
  doom: "framebuffer",
  quake: "framebuffer",
  duke3d: "framebuffer",
  nes: "framebuffer",
  "egui-kitchen-sink": "tri2d",
  "gpu-cube": "webgpu-raster",
  "scene-lab": "webgpu-raster",
};

for (const [app, profile] of Object.entries(expected)) {
  test(`${app} is a strict App manifest v2 PolkaVM product`, async () => {
    const source = await readFile(
      resolve(root, "apps", app, "manifest.json"),
      "utf8",
    );
    const manifest = JSON.parse(source);
    assert.equal(manifest.$v, 2);
    assert.equal(manifest.kind, "app");
    assert.deepEqual(Object.keys(manifest).sort(), [
      "$v",
      "appVersion",
      "capabilities",
      "kind",
      "runtime",
    ]);
    assert.equal(manifest.runtime.kind, "polkavm");
    assert.equal(manifest.runtime.abiVersion, 1);
    assert.match(manifest.runtime.entrypoint, /^[^/].*\.polkavm$/);
    assert.equal(manifest.capabilities.graphics.abiVersion, 1);
    assert.equal(manifest.capabilities.graphics.profile, profile);
    assert.deepEqual(manifest.capabilities.graphics.requiredFeatures, []);
    assert.equal("modalities" in manifest, false);
    assert.equal("contentSlots" in manifest, false);
  });

  test(`${app} deployment config uses its manifest as the only version source`, async () => {
    const config = (
      await import(
        `${pathToFileURL(resolve(root, "apps", app, "bulletin-deploy.config.mjs")).href}?test=${Date.now()}`
      )
    ).default;
    assert.equal(config.executables.length, 1);
    assert.equal(config.executables[0].kind, "app");
    assert.equal(config.executables[0].path, "./bundle");
    assert.equal(config.executables[0].manifest.$v, 2);
    assert.equal("appVersion" in config.executables[0], false);
  });
}

test("default game data comes from redistributable content packages", async () => {
  const doom = await readFile(resolve(root, "apps/doom/package.sh"), "utf8");
  const duke = await readFile(resolve(root, "apps/duke3d/package.sh"), "utf8");
  const quake = await readFile(resolve(root, "apps/quake/package.sh"), "utf8");
  const quakePatch = await readFile(
    resolve(root, "apps/quake/polkaports.patch"),
    "utf8",
  );
  assert.match(doom, /content\/freedoom\/package\.sh/);
  assert.match(duke, /content\/duke3d-shareware\/package\.sh/);
  assert.match(quake, /content\/librequake\/package\.sh/);
  assert.match(quakePatch, /^\+.*map start\\n/m);
  assert.match(quakePatch, /^-.*map e1m1\\n/m);
  assert.doesNotMatch(doom, /\bdoom1\.wad\b/);
  assert.doesNotMatch(duke, /content\/libresector\/package\.sh/);
  assert.doesNotMatch(quake, /\bid1\/pak[01]\.pak\b/);
});
