import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "quake.paseo",
  displayName: "Quake",
  description:
    "The deployed PolkaPorts Quake engine packaged with LibreQuake Lite.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", manifest }],
};
