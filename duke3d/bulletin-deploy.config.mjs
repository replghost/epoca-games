export default {
  domain: process.env.MANIFEST_DOMAIN ?? "dn3dengine.paseo",
  displayName: "Duke Nukem 3D Engine",
  description:
    "Run the Duke Nukem 3D shareware episode in Epoca with a sandboxed PolkaVM engine.",
  icon: { path: "./bundle/icon.png", format: "png" },
  executables: [
    {
      kind: "app",
      path: "./bundle",
      appVersion: [0, 1, 0],
    },
  ],
};
