#!/usr/bin/env node
// Compares what the backend resolves to in the two lock files: every package in the dependency
// closure of `ide-host` must have the same version in `app-gpui/Cargo.lock` as in the root
// `Cargo.lock`, or the two apps would be compared on different backends.
// Run from the repository root: `node docs/gpui-port/lockcheck.mjs`.
import { readFileSync } from "node:fs";

function closure(lockFile, rootPackage) {
  const packages = [];
  for (const block of readFileSync(lockFile, "utf8").split("[[package]]").slice(1)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    const deps = /^dependencies = \[([^\]]*)\]/m.exec(block)?.[1] ?? "";
    packages.push({ name, version, deps: [...deps.matchAll(/"([^"]+)"/g)].map((m) => m[1].split(" ")) });
  }
  const find = ([name, version]) => packages.filter((p) => p.name === name && (!version || p.version === version));
  const seen = new Map();
  const queue = find([rootPackage]);
  if (!queue.length) throw new Error(`${lockFile}: no package ${rootPackage}`);
  while (queue.length) {
    const p = queue.pop();
    const key = `${p.name} ${p.version}`;
    if (seen.has(key)) continue;
    seen.set(key, p);
    for (const dep of p.deps) queue.push(...find(dep));
  }
  const versions = new Map();
  for (const p of seen.values()) versions.set(p.name, [...(versions.get(p.name) ?? []), p.version].sort());
  return versions;
}

// Versions the GPUI stack forces on packages the backend also uses. Resolving the lock takes
// gpui-kit's optional `gpui-fast-web` into account (it is never compiled here), which requires
// `icu_* ~2.4.0` and `unicode-properties =0.1.3`; the root lock has ICU 2.3 and 0.1.4.
const FORCED = new Set([
  "icu_collections",
  "icu_locale_core",
  "icu_normalizer",
  "icu_normalizer_data",
  "icu_properties",
  "icu_properties_data",
  "icu_provider",
  "tinystr",
  "yoke",
  "zerovec",
  "zerovec-derive",
  "unicode-properties",
]);

const base = closure("Cargo.lock", "ide-host");
const port = closure("app-gpui/Cargo.lock", "ide-host");
const differences = [];
// Dev-dependencies of the backend crates are in the root lock only, and feature unification with
// GPUI's own dependencies adds packages to the port's closure; neither changes what is compiled
// into the backend. A package both builds use at different versions does.
let rootOnly = 0;
for (const [name, versions] of base) {
  const other = port.get(name);
  if (!other) rootOnly++;
  // The port's closure may use fewer versions of a package (no dev-dependencies), never another one.
  else if (!other.every((v) => versions.includes(v)) && !FORCED.has(name))
    differences.push(`${name}: ${versions.join(", ")} in Cargo.lock, ${other.join(", ")} in app-gpui/Cargo.lock`);
}
const portOnly = [...port.keys()].filter((name) => !base.has(name)).length;
console.log(`${rootOnly} packages only in the root closure (dev-dependencies), ${portOnly} only in the port's`);
console.log(`backend closure: ${base.size} packages in Cargo.lock, ${port.size} in app-gpui/Cargo.lock`);
if (differences.length) {
  console.error(differences.join("\n"));
  process.exit(1);
}
console.log(
  `the backend resolves to the same versions in both lock files, except ${FORCED.size} packages the GPUI stack forces`,
);
