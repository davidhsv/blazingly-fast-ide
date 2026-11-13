#!/usr/bin/env node
// Regenerates INVENTORY.md from the webview UI's sources. Every row is an item the GPUI port
// must reproduce; `parity.json` (row key → { status: "done" | "partial", proof }) marks what the
// Rust tests demonstrate. Run from the repository root: `node docs/gpui-port/inventory.mjs`.
//
// Ids come from the TypeScript program (object literals resolved through identifiers, arrays and
// helper functions), not from text patterns, and every section is cross-checked against an
// independent count: the run fails rather than write an inventory with a hole in it.
import { readFileSync, readdirSync, statSync, writeFileSync, existsSync } from "node:fs";
import { join, relative } from "node:path";
import { spawnSync } from "node:child_process";
import {
  root,
  rel,
  appSources,
  walk,
  lineCount,
  isSource,
  actions,
  extensionPoints,
  POINT_FILES,
  COVERED_ELSEWHERE,
} from "./tsmodel.mjs";

const out = join(root, "docs/gpui-port/INVENTORY.md");
const parityPath = join(root, "docs/gpui-port/parity.json");
const parity = existsSync(parityPath) ? JSON.parse(readFileSync(parityPath, "utf8")) : {};
const problems = [];

// ---------------------------------------------------------------------------------------------
// Text-level inventories (keymaps, context keys, IPC, events, plugin API, tests).

function keymapBindings() {
  const dir = join(root, "src/platform/keymaps");
  const maps = {};
  for (const name of readdirSync(dir)) {
    if (!/^(intellij|vscode|eclipse)-(macos|windows)\.ts$/.test(name)) continue;
    const text = readFileSync(join(dir, name), "utf8");
    const m = {};
    for (const line of text.matchAll(/^\s*"([\w.-]+)":\s*\[([^\]]*)\]/gm)) {
      m[line[1]] = [...line[2].matchAll(/"([^"]+)"/g)].map((b) => b[1]);
    }
    maps[name.replace(/\.ts$/, "")] = m;
  }
  return maps;
}

function contextKeys() {
  const set = new Map();
  const note = (key, file) => {
    if (["true", "false", "in", "not"].includes(key) || /^\d/.test(key)) return;
    if (!set.has(key)) set.set(key, file);
  };
  for (const source of appSources) {
    const file = rel(source.fileName);
    const text = source.text;
    for (const m of text.matchAll(/\bsetContext\(\s*["'`]([^"'`$]+)["'`]/g)) note(m[1], file);
    for (const m of text.matchAll(/\bwhen:\s*["'`]([^"'`]+)["'`]/g)) {
      // Operands only: quoted right-hand sides of `==` are values, not keys.
      const expression = m[1].replace(/(==|!=|=~)\s*('[^']*'|[^\s&|)]+)/g, "");
      for (const key of expression.matchAll(/[A-Za-z_][\w.\-:/]*/g)) note(key[0], file);
    }
  }
  return set;
}

function ipcCommands() {
  const bindings = readFileSync(join(root, "src/bindings.ts"), "utf8");
  const typed = new Set([...bindings.matchAll(/__TAURI_INVOKE(?:<[^(]*>)?\("([a-z0-9_]+)"/g)].map((m) => m[1]));
  const rows = new Map();
  const dir = join(root, "src-tauri/src");
  for (const file of walk(dir, (p) => p.endsWith(".rs"))) {
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(
      /#\[tauri::command[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g,
    )) {
      const start = m.index;
      const body = text.slice(start, text.indexOf("\n}\n", start) + 1);
      const shared = /ide_host::ops::|\bops::[a-z_]+::/.test(body);
      rows.set(m[1], { file: rel(file), typed: typed.has(m[1]), shared });
    }
  }
  const attributes = walk(dir, (p) => p.endsWith(".rs")).reduce(
    (n, f) => n + [...readFileSync(f, "utf8").matchAll(/^\s*#\[tauri::command/gm)].length,
    0,
  );
  if (rows.size !== attributes)
    problems.push(`IPC: ${attributes} #[tauri::command] attributes but ${rows.size} parsed`);
  for (const name of typed)
    if (!rows.has(name)) problems.push(`IPC: ${name} is in bindings.ts but has no #[tauri::command]`);
  return rows;
}

function ideEvents() {
  const text = readFileSync(join(root, "crates/ide-core/src/event.rs"), "utf8");
  const start = text.indexOf("pub enum IdeEvent");
  const end = text.indexOf("\n}", start);
  return [...text.slice(start, end).matchAll(/^ {4}([A-Z][A-Za-z]+)/gm)].map((m) => m[1]);
}

function settingsPages(points) {
  const found = new Map(points.get("registerSettingsPage") ?? []);
  const dir = join(root, "src/features/settings/pages");
  for (const name of readdirSync(dir)) {
    if (!isSource(join(dir, name))) continue;
    const text = readFileSync(join(dir, name), "utf8");
    for (const m of text.matchAll(/^\s+id:\s*"([a-zA-Z][\w.]*)",?\s*$/gm)) {
      if (!found.has(m[1])) found.set(m[1], `src/features/settings/pages/${name}`);
    }
  }
  return found;
}

function witInterfaces() {
  const dir = join(root, "wit");
  const items = [];
  for (const file of walk(dir, (p) => p.endsWith(".wit"))) {
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(/^\s*interface\s+([\w-]+)/gm)) items.push({ name: m[1], file: rel(file) });
  }
  return items;
}

const CASE = /^\s*(?:it|test)(?:\.(?:each|skipIf|runIf|only|skip|fails|concurrent)\b[^\n]*?)?\s*\(/gm;
function testFiles(dir, accept) {
  return walk(join(root, dir), accept)
    .map((file) => ({ file: rel(file), cases: [...readFileSync(file, "utf8").matchAll(CASE)].length }))
    .sort((a, b) => a.file.localeCompare(b.file));
}

function area(file) {
  const parts = file.split("/");
  if (parts[1] === "features" || parts[1] === "editor") return parts.slice(1, 3).join("/");
  return parts[1];
}

// ---------------------------------------------------------------------------------------------
// Rendering.

// Which parts of the GPUI app the application reaches. A unit is a top-level module of
// `app-gpui/src` or, inside a module that is a directory, one of its children. A unit is reached
// when code outside tests of an already reached unit names it; the application starts at `main`,
// `lib` and the workbench. A test proves a behaviour of the app only when its unit is reached:
// until then it proves a model that nothing calls.
const gpuiSrc = join(root, "app-gpui/src");
const withoutTests = (file) => {
  if (file.endsWith("/tests.rs") || file.includes("/tests/")) return "";
  const text = readFileSync(file, "utf8");
  const at = text.indexOf("#[cfg(test)]");
  return at < 0 ? text : text.slice(0, at);
};
// Only files the crate declares are part of it: a file on disk that no `mod` line names is not
// compiled, so it can neither be the application nor make anything reached.
const declared = new Set();
const declare = (file, childDir) => {
  if (declared.has(file) || !existsSync(file)) return;
  declared.add(file);
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;/gm)) {
    const flat = join(childDir, `${m[1]}.rs`);
    if (existsSync(flat)) declare(flat, join(childDir, m[1]));
    else declare(join(childDir, m[1], "mod.rs"), join(childDir, m[1]));
  }
};
declare(join(gpuiSrc, "lib.rs"), gpuiSrc);
declare(join(gpuiSrc, "main.rs"), gpuiSrc);
const rustFiles = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? rustFiles(path) : declared.has(path) ? [path] : [];
  });
const unitCode = new Map();
for (const name of readdirSync(gpuiSrc)) {
  const path = join(gpuiSrc, name);
  const top = name.replace(/\.rs$/, "");
  const add = (unit, files) => {
    const code = files.filter((f) => declared.has(f)).map(withoutTests);
    if (code.length) unitCode.set(unit, (unitCode.get(unit) ?? "") + code.join("\n"));
  };
  if (!statSync(path).isDirectory()) add(top, [path]);
  else
    for (const child of readdirSync(path)) {
      const inner = join(path, child);
      if (statSync(inner).isDirectory()) add(`${top}::${child}`, rustFiles(inner));
      else if (child === "mod.rs") add(top, [inner]);
      else if (child.endsWith(".rs")) add(`${top}::${child.replace(/\.rs$/, "")}`, [inner]);
    }
}
const reached = new Set([...unitCode.keys()].filter((u) => ["main", "lib", "workbench"].includes(u.split("::")[0])));
const names = (unit, from) => {
  const [top, child] = unit.split("::");
  const code = unitCode.get(from);
  if (!child) return from.split("::")[0] !== top && new RegExp(`\\b${top}::`).test(code);
  if (new RegExp(`\\b${top}::${child}\\b|\\b${top}::\\{[^}]*\\b${child}\\b`).test(code)) return true;
  return (
    from.split("::")[0] === top &&
    new RegExp(`\\b(?:super|self)::${child}\\b|\\b(?:super|self)::\\{[^}]*\\b${child}\\b|\\b${child}::`).test(code)
  );
};
for (let grew = true; grew;) {
  grew = false;
  for (const unit of unitCode.keys())
    if (!reached.has(unit) && [...reached].some((from) => names(unit, from))) {
      reached.add(unit);
      grew = true;
    }
}
const unitOf = (test) => {
  const [top, child] = test.split("::");
  return unitCode.has(`${top}::${child}`) ? `${top}::${child}` : top;
};
const citedTests = (proof) => [...(proof ?? "").matchAll(/`([a-z_]\w*(?:::\w+)+)`/g)].map((m) => m[1]);

const status = (key) => {
  const p = parity[key];
  if (!p) return { mark: "[ ]", proof: "" };
  const cited = citedTests(p.proof);
  const modelOnly = p.status === "done" && cited.length > 0 && cited.every((t) => !reached.has(unitOf(t)));
  const mark = modelOnly ? "[m]" : p.status === "done" ? "[x]" : p.status === "partial" ? "[~]" : "[ ]";
  return { mark, proof: p.proof ?? "" };
};

const sections = [];
const summary = [];
const usedKeys = new Set();
function section(title, key, rows, header, { allowEmpty = false, note = "" } = {}) {
  if (!rows.length && !allowEmpty) problems.push(`section "${title}" is empty`);
  for (const r of rows) usedKeys.add(`${key}:${r.key}`);
  const done = rows.filter((r) => status(`${key}:${r.key}`).mark === "[x]").length;
  const partial = rows.filter((r) => status(`${key}:${r.key}`).mark === "[~]").length;
  const model = rows.filter((r) => status(`${key}:${r.key}`).mark === "[m]").length;
  summary.push({ title, total: rows.length, done, partial, model });
  const counts = `${done}/${rows.length} at parity${model ? `, ${model} model only` : ""}${partial ? `, ${partial} partial` : ""}`;
  const lines = [`## ${title} (${counts})`, ""];
  if (note) lines.push(note, "");
  lines.push(`| | ${header.join(" | ")} | Proof |`, `|---|${header.map(() => "---").join("|")}|---|`);
  for (const r of rows) {
    const s = status(`${key}:${r.key}`);
    lines.push(`| ${s.mark} | ${r.cells.map((c) => String(c).replace(/\|/g, "\\|")).join(" | ")} | ${s.proof} |`);
  }
  sections.push(lines.join("\n"));
}
const code = (s) => `\`${s}\``;
const sorted = (map) => [...map].sort(([a], [b]) => a.localeCompare(b));

const actionMap = actions();
const keymaps = keymapBindings();
const defaultMap = keymaps["intellij-macos"] ?? {};
section(
  "Actions",
  "action",
  sorted(actionMap).map(([id, file]) => ({
    key: id,
    cells: [code(id), (defaultMap[id] ?? []).map(code).join(", "), file],
  })),
  ["Action id", "intellij-macos binding", "Source"],
  {
    note: "`(dynamic)` rows are actions whose id is built at run time (one per plugin command, tool, run configuration…).",
  },
);

const bound = new Map();
for (const [preset, map] of Object.entries(keymaps))
  for (const id of Object.keys(map)) if (!actionMap.has(id)) bound.set(id, [...(bound.get(id) ?? []), preset]);
section(
  "Keymap ids with no statically registered action",
  "boundid",
  sorted(bound).map(([id, presets]) => ({ key: id, cells: [code(id), presets.join(", ")] })),
  ["Id", "Presets"],
  {
    allowEmpty: true,
    note: "Bound in a keymap preset but registered at run time (dynamic rows above) or handled by the editor's own keymap.",
  },
);

section(
  "Keymap presets",
  "keymap",
  Object.entries(keymaps).map(([name, m]) => ({
    key: name,
    cells: [code(name), Object.keys(m).length, Object.values(m).reduce((n, b) => n + b.length, 0)],
  })),
  ["Preset", "Actions bound", "Bindings"],
);
if (Object.keys(keymaps).length !== 6) problems.push(`expected 6 keymap presets, found ${Object.keys(keymaps).length}`);

section(
  "Context keys",
  "context",
  sorted(contextKeys()).map(([k, file]) => ({ key: k, cells: [code(k), file] })),
  ["Key", "First source"],
);

const points = extensionPoints();
const POINT_TITLES = {
  registerToolWindow: "Tool windows",
  registerStatusItem: "Status bar items",
  registerTitleItem: "Title bar items",
  registerOverlay: "Overlays",
  registerEditorKind: "Editor kinds",
  registerFileEditorProvider: "File editor providers",
  registerSettingsPage: null, // its own section below, merged with the pages table
};
for (const [point, rows] of sorted(points)) {
  if (POINT_TITLES[point] === null) continue;
  const title = POINT_TITLES[point] ?? `Extension point \`${point}\``;
  section(
    title,
    `point.${point}`,
    sorted(rows).map(([id, file]) => ({ key: id, cells: [code(id), file] })),
    ["Id", "Registered at"],
  );
}
// Every exported register* function of the extension-point files is either used or reported.
for (const source of appSources) {
  if (!POINT_FILES.test(rel(source.fileName))) continue;
  for (const m of source.text.matchAll(/^export (?:function|const) (register[A-Z]\w*)/gm)) {
    if (!COVERED_ELSEWHERE.has(m[1]) && !points.has(m[1]))
      section(
        `Extension point \`${m[1]}\``,
        `point.${m[1]}`,
        [{ key: "(no static registrations)", cells: ["(registered at run time only)", rel(source.fileName)] }],
        ["Id", "Declared in"],
      );
  }
}

section(
  "Settings pages",
  "settings",
  sorted(settingsPages(points)).map(([id, file]) => ({ key: id, cells: [code(id), file] })),
  ["Page id", "Source"],
);

const featureDir = join(root, "src/features");
section(
  "Features",
  "feature",
  readdirSync(featureDir)
    .filter((n) => statSync(join(featureDir, n)).isDirectory())
    .sort()
    .map((n) => ({
      key: n,
      cells: [code(n), walk(join(featureDir, n), isSource).reduce((sum, f) => sum + lineCount(f), 0)],
    })),
  ["Feature", "TS lines"],
);

const moduleRows = [];
for (const top of ["src/editor", "src/platform", "src/workbench", "src/ui"]) {
  const groups = new Map();
  for (const file of walk(join(root, top), isSource)) {
    const parts = rel(file).split("/");
    // The editor is grouped two levels deep (core/view, lsp/semantic, ...), the rest per file.
    const group = top === "src/editor" && parts.length > 4 ? parts.slice(0, 4).join("/") : rel(file);
    groups.set(group, (groups.get(group) ?? 0) + lineCount(file));
  }
  for (const [group, lines] of sorted(groups)) moduleRows.push({ key: group, cells: [code(group), lines] });
}
section("Editor, platform, workbench and design-system modules", "module", moduleRows, ["Module", "TS lines"], {
  note: "Outside `src/features/`: the text editor (CodeMirror today), the platform layer, the workbench shell and the `src/ui` primitives.",
});

const tokens = readFileSync(join(root, "src/styles/tokens.css"), "utf8");
const tokenNames = new Set([...tokens.matchAll(/^\s*(--[\w-]+)\s*:/gm)].map((m) => m[1]));
// The `ICONS` table as `gen-data.mjs` resolved it from the TypeScript program.
const iconNames = new Set(
  Object.keys(JSON.parse(readFileSync(join(root, "app-gpui/assets/gen/icons.json"), "utf8")).icons),
);
const localeDir = join(root, "src/platform/locales/text");
section(
  "Design tokens, icons and locales",
  "design",
  [
    { key: "tokens", cells: ["`src/styles/tokens.css` custom properties", tokenNames.size] },
    { key: "icons", cells: ["`src/ui/icons.tsx` icons", iconNames.size] },
    ...readdirSync(localeDir)
      .filter((n) => n.endsWith(".json"))
      .map((n) => ({
        key: `locale.${n}`,
        cells: [`text catalog \`${n}\``, Object.keys(JSON.parse(readFileSync(join(localeDir, n), "utf8"))).length],
      })),
  ],
  ["Item", "Entries"],
);
if (tokenNames.size < 50) problems.push(`only ${tokenNames.size} design tokens parsed`);
if (iconNames.size < 300) problems.push(`only ${iconNames.size} icons found`);

const shellDir = join(root, "src-tauri/src");
section(
  "Native shell modules (src-tauri)",
  "shell",
  readdirSync(shellDir)
    .filter((n) => n.endsWith(".rs") && !["lib.rs", "main.rs", "state.rs", "events.rs"].includes(n))
    .sort()
    .map((n) => ({ key: n, cells: [code(`src-tauri/src/${n}`), lineCount(join(shellDir, n))] })),
  ["Module", "Rust lines"],
  {
    note: "Desktop-shell behaviour implemented in Rust next to the webview (menus, title bar, window groups, updates, CLI…).",
  },
);

const ipc = ipcCommands();
section(
  "IPC commands",
  "ipc",
  sorted(ipc).map(([name, c]) => ({
    key: name,
    cells: [code(name), c.file, c.shared ? "`ide_host::ops`" : "desktop shell", c.typed ? "yes" : "no"],
  })),
  ["Command", "Adapter", "Implementation", "In bindings.ts"],
  {
    note: "The GPUI app calls the implementation directly; a row is at parity when the UI behaviour that used the command is.",
  },
);

section(
  "Backend events (IdeEvent)",
  "event",
  ideEvents().map((e) => ({ key: e, cells: [code(e)] })),
  ["Variant"],
);
section(
  "Plugin API interfaces (wit/)",
  "wit",
  witInterfaces().map((w) => ({ key: `${w.file}#${w.name}`, cells: [code(w.name), w.file] })),
  ["Interface", "File"],
);

const unit = testFiles("src", (p) => /\.test\.tsx?$/.test(p));
const byArea = new Map();
for (const t of unit) byArea.set(area(t.file), [...(byArea.get(area(t.file)) ?? []), t]);
section(
  "Behaviour specification: unit tests (vitest)",
  "unit",
  unit.map((t) => ({ key: t.file, cells: [code(t.file), t.cases] })),
  ["Test file", "Cases"],
  {
    note:
      `${unit.length} files, ${unit.reduce((n, t) => n + t.cases, 0)} cases. A row is at parity when the behaviour its cases assert is covered by Rust tests of the GPUI app.\n\n` +
      "| Area | Files | Cases |\n|---|---|---|\n" +
      sorted(byArea)
        .map(([a, ts_]) => `| \`${a}\` | ${ts_.length} | ${ts_.reduce((n, t) => n + t.cases, 0)} |`)
        .join("\n"),
  },
);
const e2e = testFiles("e2e", (p) => /\.spec\.ts$/.test(p));
section(
  "Behaviour specification: end-to-end specs (Playwright)",
  "e2e",
  e2e.map((t) => ({ key: t.file, cells: [code(t.file), t.cases] })),
  ["Spec", "Cases"],
);

// ---------------------------------------------------------------------------------------------
// Cross-checks against independent counts.

// Comments carry example registrations; they are not part of the inventory.
const allText = appSources.map((s) => s.text.replace(/\/\*[\s\S]*?\*\/|^\s*\/\/.*$/gm, "")).join("\n");
const textual = (fn) => [...allText.matchAll(new RegExp(`\\b${fn}\\b`, "g"))].length;
for (const [point, rows] of points) {
  // The declaration plus at least one use is the least the text can contain.
  if (textual(point) < 2) problems.push(`${point}: rows found but the name is used nowhere`);
  if (!rows.size) problems.push(`${point}: no rows`);
}
const literalActionIds = new Set();
for (const m of allText.matchAll(/\bregisterActions?\(\s*[[{][\s\S]{0,400}?\bid:\s*"([a-zA-Z][\w.-]*)"/g))
  literalActionIds.add(m[1]);
for (const id of literalActionIds)
  if (!actionMap.has(id)) problems.push(`action ${id} is registered in the text but missing from the Actions section`);
if (unit.length < 700) problems.push(`only ${unit.length} unit test files found`);
for (const key of Object.keys(parity))
  if (!usedKeys.has(key)) problems.push(`parity.json names an unknown row: ${key}`);
for (const [key, p] of Object.entries(parity))
  if (p.status === "done" && !p.proof) problems.push(`parity.json: ${key} is done without a proof`);

// A proof names Rust tests (`module::tests` or `module::tests::case`, in backticks): each must
// exist in the GPUI app's test binary, so a renamed or deleted test cannot keep a row checked.
const dry = process.argv.includes("--dry");
const listed = dry
  ? { status: 0, stdout: "" }
  : spawnSync("cargo", ["test", "--lib", "--", "--list"], { cwd: join(root, "app-gpui"), encoding: "utf8" });
if (listed.status !== 0)
  problems.push(`cargo test --list failed in app-gpui:\n${(listed.stderr ?? "").split("\n").slice(-8).join("\n")}`);
const rustTests = [...(listed.stdout ?? "").matchAll(/^([\w:]+): test$/gm)].map((m) => m[1]);
// Integration tests (they run the application itself) are cited as `tests::<file>::<case>`.
for (const file of dry ? [] : readdirSync(join(root, "app-gpui/tests")).filter((f) => f.endsWith(".rs"))) {
  const name = file.slice(0, -3);
  const cases = spawnSync("cargo", ["test", "--test", name, "--", "--list"], {
    cwd: join(root, "app-gpui"),
    encoding: "utf8",
  });
  if (cases.status !== 0)
    problems.push(`cargo test --test ${name} --list failed:\n${(cases.stderr ?? "").split("\n").slice(-8).join("\n")}`);
  for (const m of (cases.stdout ?? "").matchAll(/^([\w:]+): test$/gm)) rustTests.push(`tests::${name}::${m[1]}`);
}
for (const [key, p] of Object.entries(parity)) {
  const named = [...(p.proof ?? "").matchAll(/`([a-z_]\w*(?:::\w+)+)`/g)].map((m) => m[1]);
  if (p.status === "done" && !named.length) problems.push(`parity.json: the proof of ${key} names no test`);
  for (const name of named)
    if (!dry && !rustTests.some((t) => t === name || t.startsWith(`${name}::`)))
      problems.push(`parity.json: ${key} cites ${name}, which is not a test of app-gpui`);
}

// A spec is checked off on the strength of Rust tests, so there must be at least as many of them
// as the spec has cases: counted per row, and over the rows that cite the same tests, which
// cannot each count them. A proof that says why fewer tests suffice (text after ";") is exempt.
const specCases = new Map(unit.map((t) => [`unit:${t.file}`, t.cases]));
const cited = new Map();
for (const [key, p] of Object.entries(parity)) {
  if (p.status !== "done" || !specCases.has(key) || p.proof.includes(";")) continue;
  const named = [...p.proof.matchAll(/`([a-z_]\w*(?:::\w+)+)`/g)].map((m) => m[1]);
  cited.set(key, new Set(rustTests.filter((t) => named.some((n) => t === n || t.startsWith(`${n}::`)))));
}
const group = new Map([...cited.keys()].map((key) => [key, key]));
const find = (key) => (group.get(key) === key ? key : find(group.get(key)));
const owner = new Map();
for (const [key, tests] of cited)
  for (const test of tests) {
    if (owner.has(test)) group.set(find(key), find(owner.get(test)));
    else owner.set(test, key);
  }
const groups = new Map();
for (const key of cited.keys()) groups.set(find(key), [...(groups.get(find(key)) ?? []), key]);
for (const keys of groups.values()) {
  const tests = new Set(keys.flatMap((key) => [...cited.get(key)]));
  const cases = keys.reduce((n, key) => n + specCases.get(key), 0);
  if (!dry && cases > tests.size)
    problems.push(
      `${keys.map((k) => k.slice(5)).join(", ")}: ${cases} spec cases are marked done on ${tests.size} Rust tests; ` +
        "mark the rows partial or say in each proof, after a semicolon, why fewer tests cover them",
    );
}

if (problems.length) {
  console.error(problems.map((p) => `inventory: ${p}`).join("\n"));
  process.exit(1);
}

const totalRows = summary.reduce((n, s) => n + s.total, 0);
const totalDone = summary.reduce((n, s) => n + s.done, 0);
const totalPartial = summary.reduce((n, s) => n + s.partial, 0);
const totalModel = summary.reduce((n, s) => n + s.model, 0);
const header = [
  "# GPUI port — parity inventory",
  "",
  "Generated by `node docs/gpui-port/inventory.mjs` from the webview UI's sources; do not edit by hand.",
  "A row is `[x]` only when `parity.json` names a Rust test (Proof) that demonstrates it in the GPUI app; `[~]` is partial.",
  "A row is `[m]` (model only) when its tests pass but are tests of code that nothing reachable from the application uses yet: the logic exists, the app does not do it. `[m]` rows do not count as at parity.",
  "",
  `**Overall: ${totalDone}/${totalRows} rows at parity, ${totalModel} model only, ${totalPartial} partial.**`,
  "",
  "| Section | At parity | Model only | Partial | Total |",
  "|---|---|---|---|---|",
  ...summary.map((s) => `| ${s.title} | ${s.done} | ${s.model} | ${s.partial} | ${s.total} |`),
  "",
];
// A dry run counts without the test list (no build) and writes nothing.
if (!dry) writeFileSync(out, header.join("\n") + "\n" + sections.join("\n\n") + "\n");
console.log(
  `${dry ? "dry run of" : "wrote"} ${relative(root, out)}: ${totalDone}/${totalRows} rows at parity, ${totalModel} model only, ${totalPartial} partial`,
);
