import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "duke.paseo",
  displayName: "Duke Nukem 3D Engine",
  description:
    "A sandboxed PolkaVM Duke3D port packaged with the original v1.3d shareware episode.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", manifest }],
};
