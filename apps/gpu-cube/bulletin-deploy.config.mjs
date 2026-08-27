import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "gpu.paseo",
  displayName: "PolkaVM GPU Cube",
  description: "A bounded WebGPU Raster sample running as a PolkaVM App.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", manifest }],
};
