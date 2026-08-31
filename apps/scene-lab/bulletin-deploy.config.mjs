import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "scene-lab.paseo",
  displayName: "Scene Lab",
  description: "A PolkaVM WebGPU Raster scene with an egui control surface.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", manifest }],
};
