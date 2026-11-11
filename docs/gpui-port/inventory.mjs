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
import ts from "typescript";

const root = process.cwd().replaceAll("\\", "/");
const out = join(root, "docs/gpui-port/INVENTORY.md");
const parityPath = join(root, "docs/gpui-port/parity.json");
const parity = existsSync(parityPath) ? JSON.parse(readFileSync(parityPath, "utf8")) : {};
const problems = [];

const config = ts.readConfigFile(`${root}/tsconfig.json`, ts.sys.readFile);
const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
const program = ts.createProgram(parsed.fileNames, parsed.options);
const checker = program.getTypeChecker();
const rel = (file) => file.replaceAll("\\", "/").slice(root.length + 1);
const isTest = (file) => /\.test\.tsx?$|\/__tests__\/|\/__fixtures__\/|^src\/test\//.test(file);
const appSources = program
  .getSourceFiles()
  .filter((s) => rel(s.fileName).startsWith("src/") && !isTest(rel(s.fileName)) && !s.isDeclarationFile);
const at = (node) => {
  const source = node.getSourceFile();
  return `${rel(source.fileName)}:${source.getLineAndCharacterOfPosition(node.getStart()).line + 1}`;
};

function walk(dir, accept, files = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (name === "node_modules") continue;
    if (statSync(p).isDirectory()) walk(p, accept, files);
    else if (accept(p)) files.push(p);
  }
  return files;
}
const lineCount = (file) => readFileSync(file, "utf8").split("\n").length;
const isSource = (p) => /\.(ts|tsx)$/.test(p) && !isTest(rel(p));

// ---------------------------------------------------------------------------------------------
// Static resolution of contribution objects and their ids.

const unwrap = (node) => {
  while (
    node &&
    (ts.isParenthesizedExpression(node) ||
      ts.isAsExpression(node) ||
      ts.isSatisfiesExpression(node) ||
      ts.isNonNullExpression(node) ||
      ts.isTypeAssertionExpression?.(node))
  )
    node = node.expression;
  return node;
};

function declarationsOf(identifier) {
  let symbol = checker.getSymbolAtLocation(identifier);
  if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol);
  return symbol?.declarations ?? [];
}

function returnedExpressions(fn) {
  if (!fn.body) return [];
  if (!ts.isBlock(fn.body)) return [fn.body];
  const found = [];
  const visit = (node) => {
    if (ts.isFunctionLike(node) && node !== fn) return;
    if (ts.isReturnStatement(node) && node.expression) found.push(node.expression);
    ts.forEachChild(node, visit);
  };
  visit(fn.body);
  return found;
}

/** The object literals `expr` can evaluate to (through identifiers, arrays, spreads and helper calls). */
function objectLiterals(expr, depth = 0, seen = new Set()) {
  expr = unwrap(expr);
  if (!expr || depth > 8 || seen.has(expr)) return [];
  seen.add(expr);
  const next = (e) => objectLiterals(e, depth + 1, seen);
  if (ts.isObjectLiteralExpression(expr)) return [expr];
  if (ts.isArrayLiteralExpression(expr))
    return expr.elements.flatMap((e) => next(ts.isSpreadElement(e) ? e.expression : e));
  if (ts.isConditionalExpression(expr)) return [...next(expr.whenTrue), ...next(expr.whenFalse)];
  if (ts.isBinaryExpression(expr)) return [...next(expr.left), ...next(expr.right)];
  if (ts.isIdentifier(expr)) {
    return declarationsOf(expr).flatMap((d) => {
      if (ts.isVariableDeclaration(d) && d.initializer) return next(d.initializer);
      if (ts.isParameter(d) && d.initializer) return next(d.initializer);
      return [];
    });
  }
  if (ts.isArrowFunction(expr) || ts.isFunctionExpression(expr)) return returnedExpressions(expr).flatMap(next);
  if (ts.isCallExpression(expr)) {
    const callee = unwrap(expr.expression);
    // `list.map((x) => ({ ... }))`, `list.filter(...)`, `Object.values(table)`, ...
    if (ts.isPropertyAccessExpression(callee)) {
      const method = callee.name.text;
      if (["map", "flatMap"].includes(method) && expr.arguments[0]) {
        const mapped = next(expr.arguments[0]);
        return mapped.length ? mapped : next(callee.expression);
      }
      if (["filter", "concat", "slice", "sort", "reverse"].includes(method))
        return [...next(callee.expression), ...expr.arguments.flatMap(next)];
      if (["values", "freeze"].includes(method) && expr.arguments[0]) return next(expr.arguments[0]);
    }
    if (ts.isIdentifier(callee)) {
      return declarationsOf(callee).flatMap((d) => {
        if (ts.isFunctionDeclaration(d)) return returnedExpressions(d).flatMap(next);
        if (ts.isVariableDeclaration(d) && d.initializer) {
          const init = unwrap(d.initializer);
          if (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) return returnedExpressions(init).flatMap(next);
        }
        return [];
      });
    }
  }
  return [];
}

/** String values a property initializer can take; empty when it is computed at run time. */
function literals(node, depth = 0) {
  node = unwrap(node);
  if (!node || depth > 6) return [];
  if (ts.isStringLiteralLike(node)) return [node.text];
  if (ts.isConditionalExpression(node))
    return [...literals(node.whenTrue, depth + 1), ...literals(node.whenFalse, depth + 1)];
  if (ts.isIdentifier(node)) {
    const fromDeclaration = declarationsOf(node).flatMap((d) =>
      ts.isVariableDeclaration(d) && d.initializer ? literals(d.initializer, depth + 1) : [],
    );
    if (fromDeclaration.length) return fromDeclaration;
  }
  const type = checker.getTypeAtLocation(node);
  return (type.isUnion() ? type.types : [type]).flatMap((part) => (part.isStringLiteral() ? [part.value] : []));
}

function property(object, name) {
  for (const p of object.properties) {
    if (ts.isPropertyAssignment(p) && p.name.getText().replace(/["']/g, "") === name) return literals(p.initializer);
    if (ts.isShorthandPropertyAssignment(p) && p.name.text === name) return literals(p.name);
  }
  return [];
}

// ---------------------------------------------------------------------------------------------
// Actions: every object literal typed as an Action ({ id, title, run }).

function isAction(type) {
  return ["id", "title", "run"].every((name) => checker.getPropertyOfType(type, name));
}

/** Function node → its call expressions, for ids passed into a factory (`text("editor.x", …)`). */
let callSitesByFunction = null;
function callsOf(fn) {
  if (!callSitesByFunction) {
    callSitesByFunction = new Map();
    for (const source of appSources) {
      const visit = (node) => {
        if (ts.isCallExpression(node) && ts.isIdentifier(unwrap(node.expression))) {
          for (const d of declarationsOf(unwrap(node.expression))) {
            const target = ts.isVariableDeclaration(d) && d.initializer ? unwrap(d.initializer) : d;
            if (ts.isFunctionLike(target))
              callSitesByFunction.set(target, [...(callSitesByFunction.get(target) ?? []), node]);
          }
        }
        ts.forEachChild(node, visit);
      };
      visit(source);
    }
  }
  return callSitesByFunction.get(fn) ?? [];
}

/** Ids a factory's object literal takes: the literal arguments its callers pass for the `id` parameter. */
function idsFromCallers(object) {
  const p = object.properties.find(
    (q) => (ts.isPropertyAssignment(q) || ts.isShorthandPropertyAssignment(q)) && q.name.getText() === "id",
  );
  const value = p && unwrap(ts.isPropertyAssignment(p) ? p.initializer : p.name);
  if (!value || !ts.isIdentifier(value)) return [];
  const symbol = ts.isShorthandPropertyAssignment(p)
    ? checker.getShorthandAssignmentValueSymbol(p)
    : checker.getSymbolAtLocation(value);
  const parameter = symbol?.declarations?.find(ts.isParameter);
  if (!parameter || !ts.isFunctionLike(parameter.parent)) return [];
  const index = parameter.parent.parameters.indexOf(parameter);
  return callsOf(parameter.parent).flatMap((call) => (call.arguments[index] ? literals(call.arguments[index]) : []));
}

function actions() {
  const found = new Map();
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isObjectLiteralExpression(node)) {
        const type = checker.getContextualType(node) ?? checker.getTypeAtLocation(node);
        const names = new Set(node.properties.map((p) => p.name?.getText().replace(/["']/g, "")));
        if (names.has("id") && names.has("run") && isAction(type)) {
          const own = property(node, "id");
          const ids = own.length ? own : idsFromCallers(node);
          if (ids.length) {
            for (const id of ids) if (!found.has(id)) found.set(id, at(node));
          } else {
            // An id built at run time (one action per plugin command, per run configuration, ...).
            found.set(`(dynamic) ${at(node)}`, at(node));
          }
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return found;
}

// ---------------------------------------------------------------------------------------------
// Extension points: every call of an exported `register*` function of the workbench, the platform
// and the editor API (direct, or passed to `.map` / `.forEach`).

const POINT_FILES = /^src\/(workbench\/|platform\/|editor\/api\.ts|editor\/navigationProviders\.ts)/;
const COVERED_ELSEWHERE = new Set(["registerAction", "registerActions", "registerMessages"]);
const ID_PROPERTIES = ["id", "kind", "name", "slot", "language", "scheme"];

function pointName(identifier) {
  if (!ts.isIdentifier(identifier) || !/^register[A-Z]/.test(identifier.text)) return null;
  const declared = declarationsOf(identifier).some((d) => POINT_FILES.test(rel(d.getSourceFile().fileName)));
  return declared && !COVERED_ELSEWHERE.has(identifier.text) ? identifier.text : null;
}

function extensionPoints() {
  /** point → Map(row key → source) */
  const points = new Map();
  const add = (point, objects, callSite) => {
    const rows = points.get(point) ?? new Map();
    points.set(point, rows);
    const ids = objects.flatMap((o) => ID_PROPERTIES.map((p) => property(o, p)).find((v) => v.length) ?? []);
    if (ids.length) {
      for (const id of ids) if (!rows.has(id)) rows.set(id, at(callSite));
    } else {
      // A handler or provider without an id of its own: the registration site is the row.
      rows.set(`(at) ${at(callSite)}`, at(callSite));
    }
  };
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isCallExpression(node)) {
        const callee = unwrap(node.expression);
        const direct = pointName(callee);
        if (direct && node.arguments.length) {
          // The declaration itself (`export const registerX = register(...)`) is not a use.
          add(direct, objectLiterals(node.arguments[0]), node);
        } else if (ts.isPropertyAccessExpression(callee) && ["map", "forEach", "flatMap"].includes(callee.name.text)) {
          const passed = node.arguments[0] && pointName(unwrap(node.arguments[0]));
          if (passed) add(passed, objectLiterals(callee.expression), node);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return points;
}

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

const status = (key) => {
  const p = parity[key];
  if (!p) return { mark: "[ ]", proof: "" };
  return { mark: p.status === "done" ? "[x]" : p.status === "partial" ? "[~]" : "[ ]", proof: p.proof ?? "" };
};

const sections = [];
const summary = [];
const usedKeys = new Set();
function section(title, key, rows, header, { allowEmpty = false, note = "" } = {}) {
  if (!rows.length && !allowEmpty) problems.push(`section "${title}" is empty`);
  for (const r of rows) usedKeys.add(`${key}:${r.key}`);
  const done = rows.filter((r) => status(`${key}:${r.key}`).mark === "[x]").length;
  const partial = rows.filter((r) => status(`${key}:${r.key}`).mark === "[~]").length;
  summary.push({ title, total: rows.length, done, partial });
  const lines = [`## ${title} (${done}/${rows.length} at parity${partial ? `, ${partial} partial` : ""})`, ""];
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
const icons = readFileSync(join(root, "src/ui/icons.tsx"), "utf8");
const iconNames = new Set([...icons.matchAll(/^\s{2}([A-Za-z][\w]*):\s*(?:\(|<|icon\(|svg\()/gm)].map((m) => m[1]));
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

if (problems.length) {
  console.error(problems.map((p) => `inventory: ${p}`).join("\n"));
  process.exit(1);
}

const totalRows = summary.reduce((n, s) => n + s.total, 0);
const totalDone = summary.reduce((n, s) => n + s.done, 0);
const totalPartial = summary.reduce((n, s) => n + s.partial, 0);
const header = [
  "# GPUI port — parity inventory",
  "",
  "Generated by `node docs/gpui-port/inventory.mjs` from the webview UI's sources; do not edit by hand.",
  "A row is `[x]` only when `parity.json` names a Rust test (Proof) that demonstrates it in the GPUI app; `[~]` is partial.",
  "",
  `**Overall: ${totalDone}/${totalRows} rows at parity, ${totalPartial} partial.**`,
  "",
  "| Section | At parity | Partial | Total |",
  "|---|---|---|---|",
  ...summary.map((s) => `| ${s.title} | ${s.done} | ${s.partial} | ${s.total} |`),
  "",
];
writeFileSync(out, header.join("\n") + "\n" + sections.join("\n\n") + "\n");
console.log(`wrote ${relative(root, out)}: ${totalDone}/${totalRows} rows at parity, ${totalPartial} partial`);
