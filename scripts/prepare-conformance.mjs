import { cp, mkdir, rm, stat } from "node:fs/promises";
import { resolve } from "node:path";

import {
  conformanceAppPaths,
  loadConformanceMatrix,
  root,
} from "./conformance.mjs";

const output = resolve(root, "dist", "conformance");
await rm(output, { recursive: true, force: true });
await mkdir(resolve(output, "apps"), { recursive: true });
await cp(
  resolve(root, "conformance", "matrix.json"),
  resolve(output, "matrix.json"),
);

for (const app of await loadConformanceMatrix()) {
  const paths = conformanceAppPaths(app.id);
  await requirePath(paths.bundle, `${app.id} bundle`);
  await requirePath(paths.car, `${app.id} CAR`);
  const destination = resolve(output, "apps", app.id);
  await cp(paths.bundle, resolve(destination, "bundle"), { recursive: true });
  await cp(paths.car, resolve(destination, `${app.id}.car`));
}

console.log(output);

async function requirePath(path, label) {
  if (!(await stat(path).catch(() => null))) {
    throw new Error(`${label} is missing: ${path}`);
  }
}
