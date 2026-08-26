import manifest from "./manifest.json" with { type: "json" };

export default {
  domain: process.env.MANIFEST_DOMAIN ?? "egui-lab.paseo",
  displayName: "egui Kitchen Sink",
  description:
    "A PolkaVM egui demonstration rendered through the bounded Tri2D profile.",
  icon: { path: "./icon.png", format: "png" },
  executables: [{ kind: "app", path: "./bundle", manifest }],
};
