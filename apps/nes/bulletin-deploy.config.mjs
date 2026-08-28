import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "nes.paseo",
  displayName: "NES Controller Test",
  description:
    "A sandboxed FCEUmm PolkaVM app with a reproducible MIT test cartridge.",
  icon: { path: "./icon.png", format: "png" },
  executables: [
    {
      kind: "app",
      path: "./bundle",
      appVersion: manifest.appVersion,
      manifest,
    },
  ],
};
