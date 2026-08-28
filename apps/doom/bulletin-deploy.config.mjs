import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "doom.paseo",
  displayName: "DOOM",
  description: "A sandboxed PolkaVM Doom port packaged with Freedoom Phase 1.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", appVersion: manifest.appVersion, manifest }],
};
