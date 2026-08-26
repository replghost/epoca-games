import { createHash } from "node:crypto";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { merkleizeJS } from "bulletin-deploy";

const root = resolve(import.meta.dirname, "..");
const appId = process.argv[2];
if (!appId || !/^[a-z0-9][a-z0-9-]*$/.test(appId)) {
  throw new Error("usage: node scripts/prepare-app.mjs <app-id>");
}
const appDir = resolve(root, "apps", appId);
const bundleDir = resolve(appDir, "bundle");
const configModule = await import(
  `${pathToFileURL(resolve(appDir, "bulletin-deploy.config.mjs")).href}?t=${Date.now()}`
);
const config = configModule.default;
const executable = config?.executables?.find((item) => item?.kind === "app");
const manifest = executable?.manifest;

validateManifest(manifest);
if (resolve(appDir, executable.path) !== bundleDir) {
  throw new Error(`${appId}: executable path must be ./bundle`);
}
const canonicalManifest = JSON.stringify(manifest);
await writeFile(resolve(bundleDir, "manifest.json"), canonicalManifest);
await requireFile(
  resolve(bundleDir, manifest.runtime.entrypoint),
  `${appId} PolkaVM entrypoint`,
);
await requireFile(resolve(appDir, config.icon.path), `${appId} icon`);

const { carBytes, cid } = await merkleizeJS(bundleDir);
const dist = resolve(root, "dist");
await mkdir(dist, { recursive: true });
const carPath = resolve(dist, `${appId}.car`);
await writeFile(carPath, carBytes);
const release = {
  app: appId,
  domain: config.domain,
  cid,
  appVersion: manifest.appVersion,
  manifestSha256: sha256(Buffer.from(canonicalManifest)),
  carSha256: sha256(carBytes),
};
await writeFile(
  resolve(dist, `${appId}.release.json`),
  `${JSON.stringify(release, null, 2)}\n`,
);
console.log(`${appId}: ${cid}`);

function validateManifest(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${appId}: App manifest must be an object`);
  }
  const expected = ["$v", "kind", "appVersion", "runtime", "capabilities"];
  const unknown = Object.keys(value).find((key) => !expected.includes(key));
  if (unknown)
    throw new Error(`${appId}: unknown App manifest field ${unknown}`);
  if (value.$v !== 2 || value.kind !== "app") {
    throw new Error(`${appId}: manifest must be App version 2`);
  }
  if (
    !Array.isArray(value.appVersion) ||
    value.appVersion.length !== 3 ||
    value.appVersion.some((part) => !Number.isSafeInteger(part) || part < 0)
  ) {
    throw new Error(`${appId}: invalid appVersion`);
  }
  if (
    value.runtime?.kind !== "polkavm" ||
    value.runtime.abiVersion !== 1 ||
    typeof value.runtime.entrypoint !== "string" ||
    value.runtime.entrypoint.startsWith("/") ||
    value.runtime.entrypoint
      .split("/")
      .some((part) => !part || part === "." || part === "..") ||
    !value.runtime.entrypoint.endsWith(".polkavm")
  ) {
    throw new Error(`${appId}: invalid PolkaVM runtime`);
  }
  const graphics = value.capabilities?.graphics;
  if (
    graphics?.abiVersion !== 1 ||
    !["framebuffer", "tri2d", "webgpu-raster"].includes(graphics.profile) ||
    !Array.isArray(graphics.requiredFeatures) ||
    graphics.requiredFeatures.length !== 0
  ) {
    throw new Error(`${appId}: unsupported graphics requirements`);
  }
  validateCapability(
    value.capabilities.deviceInput,
    new Set(["pointer", "keyboard", "touch", "wheel", "text", "ime", "focus"]),
    "deviceInput",
  );
  validateCapability(value.capabilities.audio, new Set(), "audio");
}

function validateCapability(value, allowed, label) {
  if (value === undefined) return;
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    value.abiVersion !== 1 ||
    !Array.isArray(value.requiredFeatures) ||
    new Set(value.requiredFeatures).size !== value.requiredFeatures.length ||
    value.requiredFeatures.some(
      (feature) => typeof feature !== "string" || !allowed.has(feature),
    )
  ) {
    throw new Error(`${appId}: unsupported ${label} requirements`);
  }
}

async function requireFile(path, label) {
  const details = await stat(path).catch(() => null);
  if (!details?.isFile()) throw new Error(`${label} is missing: ${path}`);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
