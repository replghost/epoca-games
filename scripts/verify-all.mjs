import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { CarReader } from "@ipld/car";
import * as dagPb from "@ipld/dag-pb";
import { UnixFS } from "ipfs-unixfs";

const root = resolve(import.meta.dirname, "..");
const apps = [
  "doom",
  "quake",
  "duke3d",
  "nes",
  "egui-kitchen-sink",
  "gpu-cube",
  "scene-lab",
];
const compatibility = [];
for (const app of apps) {
  const appDir = resolve(root, "apps", app);
  const config = (
    await import(
      `${pathToFileURL(resolve(appDir, "bulletin-deploy.config.mjs")).href}?verify=${Date.now()}`
    )
  ).default;
  const executable = config.executables.find((item) => item.kind === "app");
  const expectedManifest = Buffer.from(JSON.stringify(executable.manifest));
  const carBytes = new Uint8Array(
    await readFile(resolve(root, "dist", `${app}.car`)),
  );
  const files = await extractCar(carBytes);
  const embedded = files.get("manifest.json");
  if (!embedded || !Buffer.from(embedded).equals(expectedManifest)) {
    throw new Error(
      `${app}: embedded manifest bytes differ from executable record bytes`,
    );
  }
  if (!files.has(executable.manifest.runtime.entrypoint)) {
    throw new Error(
      `${app}: CAR is missing ${executable.manifest.runtime.entrypoint}`,
    );
  }
  verifyLicensing(app, files);
  const release = JSON.parse(
    await readFile(resolve(root, "dist", `${app}.release.json`), "utf8"),
  );
  if (release.manifestSha256 !== sha256(expectedManifest)) {
    throw new Error(`${app}: release manifest digest is stale`);
  }
  if (release.carSha256 !== sha256(carBytes)) {
    throw new Error(`${app}: release CAR digest is stale`);
  }
  const profile = executable.manifest.capabilities.graphics.profile;
  compatibility.push({
    app,
    profile,
    hosts: {
      epoca: app === "nes" ? "known-init-trap" : "launch",
      dotli: "launch",
    },
  });
  console.log(`${app}: verified ${files.size} files (${profile})`);
}
await writeFile(
  resolve(root, "dist", "compatibility.json"),
  `${JSON.stringify(compatibility, null, 2)}\n`,
);

async function extractCar(bytes) {
  const reader = await CarReader.fromBytes(bytes);
  const roots = await reader.getRoots();
  if (roots.length !== 1) throw new Error("CAR must have exactly one root");
  const blocks = new Map();
  for await (const block of reader.blocks())
    blocks.set(block.cid.toString(), block.bytes);
  const files = new Map();

  async function content(cid) {
    const bytesForCid = blocks.get(cid.toString());
    if (!bytesForCid) throw new Error(`CAR is missing block ${cid}`);
    if (cid.code === 0x55) return bytesForCid;
    if (cid.code !== 0x70) throw new Error(`unsupported CAR codec ${cid.code}`);
    const node = dagPb.decode(bytesForCid);
    const unixfs = node.Data ? UnixFS.unmarshal(node.Data) : null;
    if (
      unixfs?.type === "directory" ||
      unixfs?.type === "hamt-sharded-directory"
    ) {
      throw new Error("directory block used as file content");
    }
    const chunks = [unixfs?.data ?? new Uint8Array()];
    for (const link of node.Links) chunks.push(await content(link.Hash));
    return concat(chunks);
  }

  async function walk(cid, prefix) {
    const bytesForCid = blocks.get(cid.toString());
    if (!bytesForCid) throw new Error(`CAR is missing block ${cid}`);
    if (cid.code === 0x55) {
      if (!prefix)
        throw new Error("raw CAR root cannot describe an app directory");
      files.set(prefix, bytesForCid);
      return;
    }
    const node = dagPb.decode(bytesForCid);
    const unixfs = node.Data ? UnixFS.unmarshal(node.Data) : null;
    if (
      unixfs?.type !== "directory" &&
      unixfs?.type !== "hamt-sharded-directory"
    ) {
      if (!prefix) throw new Error("CAR root must be a directory");
      files.set(prefix, await content(cid));
      return;
    }
    for (const link of node.Links) {
      if (
        !link.Name ||
        link.Name.includes("/") ||
        link.Name === "." ||
        link.Name === ".."
      ) {
        throw new Error(`unsafe CAR path component: ${link.Name}`);
      }
      const path = prefix ? `${prefix}/${link.Name}` : link.Name;
      if (files.has(path)) throw new Error(`duplicate CAR path: ${path}`);
      await walk(link.Hash, path);
    }
  }

  await walk(roots[0], "");
  return files;
}

function verifyLicensing(app, files) {
  const required = {
    doom: [
      "game/doom.wad",
      "LICENSES/doomgeneric-GPL-2.0.txt",
      "LICENSES/Freedoom-COPYING.txt",
    ],
    quake: [
      "pak0.pak",
      "pak1.pak",
      "LICENSES/Quake-GPL-2.0-or-later.txt",
      "LICENSES/LibreQuake-BSD-3-Clause.txt",
      "LICENSES/LibreQuake-GPL-2.0.txt",
    ],
    duke3d: [
      "game/duke3d.grp",
      "DISTRIBUTION/3dduke13.zip",
      "LICENSES/Duke3D-GPL-2.0-or-later.txt",
      "LICENSES/Duke3D-shareware-LICENSE.txt",
      "SOURCES/Duke3D-shareware.json",
    ],
    nes: [
      "game/cartridge.nes",
      "LICENSES/FCEUmm-GPL-2.0.txt",
      "LICENSES/controller-test-MIT.txt",
    ],
    "egui-kitchen-sink": ["LICENSES/MPL-2.0.txt"],
    "gpu-cube": ["LICENSES/MPL-2.0.txt"],
    "scene-lab": ["showcase.epm", "LICENSES/MPL-2.0.txt"],
  }[app];
  for (const path of required) {
    if (!files.has(path))
      throw new Error(`${app}: required licensed asset is missing: ${path}`);
  }
}

function concat(chunks) {
  const total = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const output = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
