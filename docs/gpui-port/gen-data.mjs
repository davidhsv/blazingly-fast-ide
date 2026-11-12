#!/usr/bin/env node
// Writes the data the GPUI app shares with the webview UI to `app-gpui/assets/gen/`:
//
//   keymaps.json        the six keymap presets, evaluated from `src/platform/keymaps/*.ts`
//   actions.json        every action's static metadata (id, title, category, when, …)
//   contributions.json  every extension-point registration's static properties
//   icons.json          icon name → SVG path under `src/ui/` (the `ICONS` table)
//
// The TypeScript sources stay the source of truth while both apps exist. Run from the
// repository root: `node docs/gpui-port/gen-data.mjs` (`--check` fails when a file is stale).
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import ts from "typescript";
import {
  root,
  rel,
  checker,
  appSources,
  at,
  unwrap,
  declarationsOf,
  objectLiterals,
  literals,
  isAction,
  callsOf,
  pointName,
} from "./tsmodel.mjs";

const outDir = join(root, "app-gpui/assets/gen");
const check = process.argv.includes("--check");
const stale = [];
function emit(name, value) {
  const file = join(outDir, name);
  const text = JSON.stringify(value, null, 1) + "\n";
  if (check) {
    if (!existsSync(file) || readFileSync(file, "utf8") !== text) stale.push(name);
    return;
  }
  mkdirSync(outDir, { recursive: true });
  writeFileSync(file, text);
  console.log(`wrote app-gpui/assets/gen/${name}`);
}

// ---------------------------------------------------------------------------------------------
// Keymap presets: the modules are evaluated, so spreads and the Windows translation apply.

function keymaps() {
  const dir = join(root, "src/platform/keymaps");
  const cache = new Map();
  const load = (name) => {
    if (cache.has(name)) return cache.get(name).exports;
    const source = readFileSync(join(dir, `${name}.ts`), "utf8");
    const js = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const module = { exports: {} };
    cache.set(name, module);
    const require = (request) => {
      if (!request.startsWith("./"))
        throw new Error(`keymap ${name} imports ${request}: only sibling presets are evaluated`);
      return load(request.slice(2));
    };
    vm.runInNewContext(js, { module, exports: module.exports, require });
    return module.exports;
  };
  const presets = {};
  for (const [id, file, symbol] of [
    ["intellij-macos", "intellij-macos", "intellijMacos"],
    ["intellij-windows", "intellij-windows", "intellijWindows"],
    ["vscode-macos", "vscode-macos", "vscodeMacos"],
    ["vscode-windows", "vscode-windows", "vscodeWindows"],
    ["eclipse-macos", "eclipse-macos", "eclipseMacos"],
    ["eclipse-windows", "eclipse-windows", "eclipseWindows"],
  ]) {
    const map = load(file)[symbol];
    if (!map || Object.keys(map).length < 100) throw new Error(`keymap preset ${id} did not evaluate`);
    // In declaration order, as pairs: where two actions share a key the later one wins, and a
    // JSON object's order does not survive every reader.
    presets[id] = Object.entries(map);
  }
  return presets;
}

// ---------------------------------------------------------------------------------------------
// Static values of expressions; `env` binds a factory's parameters to a caller's arguments.

const NONE = Symbol("not static");

function staticValue(node, env, depth = 0) {
  node = unwrap(node);
  if (!node || depth > 8) return NONE;
  if (ts.isStringLiteralLike(node)) return node.text;
  if (ts.isNumericLiteral(node)) return Number(node.text);
  if (node.kind === ts.SyntaxKind.TrueKeyword) return true;
  if (node.kind === ts.SyntaxKind.FalseKeyword) return false;
  if (node.kind === ts.SyntaxKind.NullKeyword) return null;
  if (ts.isPrefixUnaryExpression(node) && node.operator === ts.SyntaxKind.MinusToken) {
    const v = staticValue(node.operand, env, depth + 1);
    return typeof v === "number" ? -v : NONE;
  }
  if (ts.isArrayLiteralExpression(node)) {
    const out = [];
    for (const e of node.elements) {
      if (ts.isSpreadElement(e)) {
        const spread = staticValue(e.expression, env, depth + 1);
        if (!Array.isArray(spread)) return NONE;
        out.push(...spread);
      } else {
        const v = staticValue(e, env, depth + 1);
        if (v === NONE) return NONE;
        out.push(v);
      }
    }
    return out;
  }
  if (ts.isObjectLiteralExpression(node)) return staticObject(node, env, depth + 1);
  if (ts.isIdentifier(node)) {
    if (node.text === "undefined") return NONE;
    const symbol = ts.isShorthandPropertyAssignment(node.parent)
      ? checker.getShorthandAssignmentValueSymbol(node.parent)
      : checker.getSymbolAtLocation(node);
    if (symbol && env?.has(symbol)) return env.get(symbol) ? staticValue(env.get(symbol), env.outer, depth + 1) : NONE;
    for (const d of declarationsOf(node)) {
      if (ts.isVariableDeclaration(d) && d.initializer && d.parent.flags & ts.NodeFlags.Const) {
        const v = staticValue(d.initializer, undefined, depth + 1);
        if (v !== NONE) return v;
      }
    }
  }
  const type = checker.getTypeAtLocation(node);
  if (type.isStringLiteral() || type.isNumberLiteral()) return type.value;
  if (type.flags & ts.TypeFlags.BooleanLiteral) return checker.typeToString(type) === "true";
  return NONE;
}

/** The statically known properties of an object literal; computed ones are left out. */
function staticObject(object, env, depth = 0) {
  const out = {};
  for (const p of object.properties) {
    if (ts.isSpreadAssignment(p)) {
      const spread = staticValue(p.expression, env, depth + 1);
      if (spread !== NONE && spread && typeof spread === "object" && !Array.isArray(spread)) Object.assign(out, spread);
      else for (const o of objectLiterals(p.expression)) Object.assign(out, staticObject(o, env, depth + 1));
      continue;
    }
    if (!p.name) continue;
    const name = ts.isComputedPropertyName(p.name) ? null : p.name.getText().replace(/^["']|["']$/g, "");
    if (!name) continue;
    if (ts.isMethodDeclaration(p) || ts.isGetAccessorDeclaration(p)) {
      out[`$${name}`] = "function";
      continue;
    }
    const initializer = ts.isPropertyAssignment(p) ? p.initializer : p.name;
    const inner = unwrap(initializer);
    if (ts.isArrowFunction(inner) || ts.isFunctionExpression(inner)) {
      out[`$${name}`] = "function";
      continue;
    }
    const value = staticValue(initializer, env, depth + 1);
    if (value !== NONE) {
      out[name] = value;
      continue;
    }
    // `t("catalog.key", args)` is a message-catalog lookup; `tx("English")` is source text.
    if (ts.isCallExpression(inner) && ts.isIdentifier(inner.expression) && inner.arguments[0]) {
      const first = staticValue(inner.arguments[0], env, depth + 1);
      if (inner.expression.text === "t" && typeof first === "string") {
        out[`${name}Key`] = first;
        continue;
      }
      if (inner.expression.text === "tx" && typeof first === "string") {
        out[name] = first;
        continue;
      }
    }
    // Several literal branches (`mac ? "Reveal in Finder" : "Show in Explorer"`): keep them all.
    const options = env ? [] : literals(initializer);
    if (options.length > 1) {
      out[name] = options[0];
      out[`${name}Variants`] = options;
    } else if (options.length === 1) out[name] = options[0];
    else out[`$${name}`] = "computed";
  }
  return out;
}

/**
 * One static record per instance of `object`: itself, or, when it is built by a factory whose
 * parameters it reads (`text("editor.x", "Title", run)`), one per call of that factory.
 */
function instances(object) {
  let fn = object.parent;
  while (fn && !ts.isFunctionLike(fn) && !ts.isSourceFile(fn)) fn = fn.parent;
  const readsParameter = () => {
    let found = false;
    const visit = (n) => {
      if (found) return;
      if (ts.isIdentifier(n)) {
        const symbol = ts.isShorthandPropertyAssignment(n.parent)
          ? checker.getShorthandAssignmentValueSymbol(n.parent)
          : checker.getSymbolAtLocation(n);
        if (symbol?.declarations?.some((d) => ts.isParameter(d) && d.parent === fn)) found = true;
      }
      if (!ts.isFunctionLike(n)) ts.forEachChild(n, visit);
    };
    for (const p of object.properties) visit(p);
    return found;
  };
  if (!fn || !ts.isFunctionLike(fn) || !readsParameter()) return [{ record: staticObject(object), site: object }];
  const calls = callsOf(fn);
  if (!calls.length) return [{ record: staticObject(object), site: object }];
  return calls.map((call) => {
    const env = new Map();
    fn.parameters.forEach((parameter, i) => {
      const symbol = checker.getSymbolAtLocation(parameter.name);
      if (symbol) env.set(symbol, call.arguments[i] ?? parameter.initializer ?? null);
    });
    return { record: staticObject(object, env), site: call };
  });
}

// ---------------------------------------------------------------------------------------------

const ACTION_FIELDS = [
  "id",
  "title",
  "category",
  "description",
  "icon",
  "when",
  "keyWhen",
  "keywords",
  "verbatim",
  "hidden",
];

function actionManifest() {
  const byId = new Map();
  const dynamic = [];
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isObjectLiteralExpression(node)) {
        const type = checker.getContextualType(node) ?? checker.getTypeAtLocation(node);
        const names = new Set(node.properties.map((p) => p.name?.getText().replace(/["']/g, "")));
        if (names.has("id") && names.has("run") && isAction(type)) {
          for (const { record, site } of instances(node)) {
            const entry = {};
            for (const field of ACTION_FIELDS) {
              if (field in record) entry[field] = record[field];
              for (const suffix of ["Variants", "Key"])
                if (`${field}${suffix}` in record) entry[`${field}${suffix}`] = record[`${field}${suffix}`];
            }
            if ("$toggled" in record) entry.toggle = true;
            entry.source = at(site);
            const ids = typeof entry.id === "string" ? (record.idVariants ?? [entry.id]) : [];
            if (!ids.length) dynamic.push({ ...entry, id: undefined, definedAt: at(node) });
            for (const id of ids) if (!byId.has(id)) byId.set(id, { ...entry, id, idVariants: undefined });
          }
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return {
    actions: [...byId.values()].sort((a, b) => a.id.localeCompare(b.id)),
    // Actions whose id is built at run time (one per plugin command, tool window, bookmark digit…).
    dynamic: dynamic.sort((a, b) => a.definedAt.localeCompare(b.definedAt)),
  };
}

function contributionManifest() {
  const points = {};
  const add = (point, objects, site) => {
    const rows = (points[point] ??= []);
    if (!objects.length) rows.push({ source: at(site) });
    for (const object of objects)
      for (const { record } of instances(object)) rows.push({ ...record, source: at(site) });
  };
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isCallExpression(node)) {
        const callee = unwrap(node.expression);
        const direct = pointName(callee);
        if (direct && node.arguments.length) add(direct, objectLiterals(node.arguments[0]), node);
        else if (ts.isPropertyAccessExpression(callee) && ["map", "forEach", "flatMap"].includes(callee.name.text)) {
          const passed = node.arguments[0] && pointName(unwrap(node.arguments[0]));
          if (passed) add(passed, objectLiterals(callee.expression), node);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return Object.fromEntries(
    Object.entries(points)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([point, rows]) => [point, rows.sort((a, b) => a.source.localeCompare(b.source, "en", { numeric: true }))]),
  );
}

/** The `ICONS` (name → SVG path) and `SYMBOL_ICONS` (symbol kind → SVG path) tables. */
function iconTables() {
  const source = appSources.find((s) => rel(s.fileName) === "src/ui/icons.tsx");
  const tables = {};
  const visit = (node) => {
    if (ts.isVariableDeclaration(node) && ["ICONS", "SYMBOL_ICONS"].includes(node.name.getText()) && node.initializer) {
      const object = objectLiterals(node.initializer)[0];
      if (object) tables[node.name.getText()] = staticObject(object);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  const { ICONS: icons, SYMBOL_ICONS: symbols } = tables;
  if (!icons || Object.keys(icons).length < 300) throw new Error("the ICONS table of src/ui/icons.tsx did not resolve");
  if (!symbols || Object.keys(symbols).length < 10)
    throw new Error("the SYMBOL_ICONS table of src/ui/icons.tsx did not resolve");
  for (const [name, path] of [...Object.entries(icons), ...Object.entries(symbols)]) {
    const file =
      typeof path === "string" && (path.startsWith("openidea/") ? `src/ui/${path}.svg` : `src/ui/expui/${path}.svg`);
    if (!file || !existsSync(join(root, file))) throw new Error(`icon ${name}: ${file} does not exist`);
  }
  const sortedEntries = (table) => Object.fromEntries(Object.entries(table).sort(([a], [b]) => a.localeCompare(b)));
  return { icons: sortedEntries(icons), symbols: sortedEntries(symbols) };
}

/**
 * What `Intl.NumberFormat` and `Intl.PluralRules` answer for the UI locales: the reference the
 * Rust number formatting and plural rules are tested against.
 */
function intlTable() {
  const numbers = [
    0, 1, 2, 3, 4, 5, 10, 11, 12, 14, 15, 21, 22, 25, 100, 101, 102, 112, 999, 1000, 1234, 9999, 10000, 12345, 123456,
    1000000, 2000000, 1234567.891, 0.5, 1.5, 2.5, 1234.5, 0.0001, 0.0005, 1e9, -1, -1234.5, -0.0001,
  ];
  const table = {};
  for (const locale of ["en", "zh", "ko", "fr", "pl", "pt", "es"]) {
    const format = new Intl.NumberFormat(locale);
    const plural = new Intl.PluralRules(locale);
    table[locale] = numbers.map((n) => ({ n, text: format.format(n), plural: plural.select(n) }));
  }
  return table;
}

/**
 * The file-name → icon tables of `src/ui/icons.tsx` (evaluated, so `table({...})` is expanded),
 * and the cases `icons.test.tsx` asserts for them, which the Rust test replays.
 */
function fileIconTables() {
  const file = join(root, "src/ui/icons.tsx");
  const source = ts.createSourceFile(file, readFileSync(file, "utf8"), ts.ScriptTarget.ES2022, true);
  const wanted = ["table", "FILE_NAMES", "NAME_PATTERNS", "EXT_ICONS", "CODE_EXT"];
  const parts = [];
  ts.forEachChild(source, (node) => {
    const name = ts.isFunctionDeclaration(node)
      ? node.name?.text
      : ts.isVariableStatement(node)
        ? node.declarationList.declarations[0]?.name.getText(source)
        : undefined;
    if (name && wanted.includes(name)) parts.push(node.getText(source));
  });
  if (parts.length !== wanted.length) throw new Error("icons.tsx no longer declares the file icon tables as expected");
  const js = ts.transpileModule(
    `${parts.join("\n")}\nexports.result = { FILE_NAMES, NAME_PATTERNS: NAME_PATTERNS.map(([re, icon]) => [re.source, icon]), EXT_ICONS, CODE_EXT: [...CODE_EXT].sort() };`,
    { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } },
  ).outputText;
  const module = { exports: {} };
  vm.runInNewContext(js, { module, exports: module.exports });
  const { FILE_NAMES, NAME_PATTERNS, EXT_ICONS, CODE_EXT } = module.exports.result;

  // `it.each([[file, icon], …])("%s → %s", …)` in the spec.
  const specFile = join(root, "src/ui/icons.test.tsx");
  const spec = ts.createSourceFile(specFile, readFileSync(specFile, "utf8"), ts.ScriptTarget.ES2022, true);
  let cases = null;
  const visit = (node) => {
    if (
      ts.isCallExpression(node) &&
      ts.isCallExpression(node.expression) &&
      node.arguments[0] &&
      ts.isStringLiteralLike(node.arguments[0]) &&
      node.arguments[0].text === "%s → %s" &&
      ts.isArrayLiteralExpression(node.expression.arguments[0])
    ) {
      const rows = node.expression.arguments[0].elements.map((row) =>
        ts.isArrayLiteralExpression(row)
          ? row.elements.map((cell) => (ts.isStringLiteralLike(cell) ? cell.text : null))
          : [],
      );
      if (
        rows.every((row) => row.length === 2 && row.every((cell) => typeof cell === "string")) &&
        rows.length > (cases?.length ?? 0)
      )
        cases = rows;
    }
    ts.forEachChild(node, visit);
  };
  visit(spec);
  if (!cases || cases.length < 50) throw new Error("the file icon cases of icons.test.tsx were not found");
  const sortedEntries = (table) => Object.fromEntries(Object.entries(table).sort(([a], [b]) => a.localeCompare(b)));
  return {
    fileNames: sortedEntries(FILE_NAMES),
    namePatterns: NAME_PATTERNS,
    extensions: sortedEntries(EXT_ICONS),
    codeExtensions: CODE_EXT,
    cases,
  };
}

/**
 * How `Intl.Collator` orders a list of awkward names under the three option sets the UI uses:
 * the reference the Rust collation is tested against.
 */
function collationTable() {
  const names = [
    "main",
    "Main",
    "MAIN",
    "a",
    "A",
    "b",
    "B",
    "a1",
    "a2",
    "a10",
    "a01",
    "A2",
    "file2.txt",
    "file10.txt",
    "file1.txt",
    "_build",
    ".gitignore",
    "-dash",
    "a_b",
    "a-b",
    "a.b",
    "a b",
    "a+b",
    "ab",
    "aB",
    "Ab",
    "~tilde",
    "{brace}",
    "@at",
    "=eq",
    ":colon",
    "1",
    "2",
    "10",
    "02",
    "résumé",
    "resume",
    "Resume",
    "é",
    "e",
    "f",
    "z",
    "ä",
    "ß",
    "ss",
    "ﬁ",
    "fi",
    "σ",
    "ς",
    "İ",
    "i",
    "I",
    "Привет",
    "αβγ",
    "中文",
    "日本",
    "😀",
    "zebra",
    "Zebra",
    "apple",
    "Apple",
    "src",
    "Src",
    "tests",
    "target",
    "README.md",
    "readme.md",
    "feature/x",
    "feature-x",
    "feature_x",
    "release/1.2",
    "release/1.10",
    "v1.9",
    "v1.10",
    "",
    " ",
  ];
  const variants = {
    default: {},
    base: { sensitivity: "base" },
    numericBase: { numeric: true, sensitivity: "base" },
    numeric: { numeric: true },
  };
  const table = { names };
  for (const [key, options] of Object.entries(variants)) {
    const collator = new Intl.Collator("en", options);
    // Indices into `names`, sorted stably: names the collator holds equal keep their order.
    table[key] = names.map((_, i) => i).sort((a, b) => collator.compare(names[a], names[b]));
    // Every pair's sign, row by row as `<`, `=`, `>`: a sorted list alone cannot tell names held
    // equal from names that happened to be listed in order.
    table[`${key}Signs`] = names.map((a) => names.map((b) => "<=>"[Math.sign(collator.compare(a, b)) + 1]).join(""));
  }
  return table;
}

emit("keymaps.json", keymaps());
emit("collation.json", collationTable());
emit("file-icons.json", fileIconTables());
emit("intl.json", intlTable());
const manifest = actionManifest();
if (manifest.actions.length < 600) throw new Error(`only ${manifest.actions.length} actions resolved`);
emit("actions.json", manifest);
emit("contributions.json", contributionManifest());
emit("icons.json", iconTables());

if (stale.length) {
  console.error(`stale: ${stale.join(", ")} — run node docs/gpui-port/gen-data.mjs`);
  process.exit(1);
}
