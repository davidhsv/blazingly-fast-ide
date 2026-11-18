// The webview UI's TypeScript program and the static resolution the port's generators share:
// which object literals an expression can evaluate to, and the literal values of their properties.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

export const root = process.cwd().replaceAll("\\", "/");

export const config = ts.readConfigFile(`${root}/tsconfig.json`, ts.sys.readFile);
export const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
export const program = ts.createProgram(parsed.fileNames, parsed.options);
export const checker = program.getTypeChecker();
export const rel = (file) => file.replaceAll("\\", "/").slice(root.length + 1);
export const isTest = (file) => /\.test\.tsx?$|\/__tests__\/|\/__fixtures__\/|^src\/test\//.test(file);
export const appSources = program
  .getSourceFiles()
  .filter((s) => rel(s.fileName).startsWith("src/") && !isTest(rel(s.fileName)) && !s.isDeclarationFile);
export const at = (node) => {
  const source = node.getSourceFile();
  return `${rel(source.fileName)}:${source.getLineAndCharacterOfPosition(node.getStart()).line + 1}`;
};

export function walk(dir, accept, files = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (name === "node_modules") continue;
    if (statSync(p).isDirectory()) walk(p, accept, files);
    else if (accept(p)) files.push(p);
  }
  return files;
}
export const lineCount = (file) => readFileSync(file, "utf8").split("\n").length;
export const isSource = (p) => /\.(ts|tsx)$/.test(p) && !isTest(rel(p));

// ---------------------------------------------------------------------------------------------
// Static resolution of contribution objects and their ids.

export const unwrap = (node) => {
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

export function declarationsOf(identifier) {
  let symbol = checker.getSymbolAtLocation(identifier);
  if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol);
  return symbol?.declarations ?? [];
}

export function returnedExpressions(fn) {
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
export function objectLiterals(expr, depth = 0, seen = new Set()) {
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
export function literals(node, depth = 0) {
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

export function property(object, name) {
  for (const p of object.properties) {
    if (ts.isPropertyAssignment(p) && p.name.getText().replace(/["']/g, "") === name) return literals(p.initializer);
    if (ts.isShorthandPropertyAssignment(p) && p.name.text === name) return literals(p.name);
  }
  return [];
}

// ---------------------------------------------------------------------------------------------
// Actions: every object literal typed as an Action ({ id, title, run }).

export function isAction(type) {
  return ["id", "title", "run"].every((name) => checker.getPropertyOfType(type, name));
}

/** Function node → its call expressions, for ids passed into a factory (`text("editor.x", …)`). */
export let callSitesByFunction = null;
export function callsOf(fn) {
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
export function idsFromCallers(object) {
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

export function actions() {
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
// String values reached through constants, parameters and helpers (context keys and `when` clauses
// that are not written as literals at the place that sets or tests them).

/** Component function → the JSX elements that render it (`<Tree focusContext="…" />`). */
let jsxByFunction = null;
export function jsxOf(fn) {
  if (!jsxByFunction) {
    jsxByFunction = new Map();
    for (const source of appSources) {
      const visit = (node) => {
        if ((ts.isJsxSelfClosingElement(node) || ts.isJsxOpeningElement(node)) && ts.isIdentifier(node.tagName)) {
          for (const d of declarationsOf(node.tagName)) {
            const target = ts.isVariableDeclaration(d) && d.initializer ? unwrap(d.initializer) : d;
            if (ts.isFunctionLike(target)) jsxByFunction.set(target, [...(jsxByFunction.get(target) ?? []), node]);
          }
        }
        ts.forEachChild(node, visit);
      };
      visit(source);
    }
  }
  return jsxByFunction.get(fn) ?? [];
}

const NOTHING = { values: [], dynamic: false };
const DYNAMIC = { values: [], dynamic: true };
const merge = (results) => ({ values: results.flatMap((r) => r.values), dynamic: results.some((r) => r.dynamic) });
const isConst = (d) => Boolean(ts.getCombinedNodeFlags(d) & ts.NodeFlags.Const);

/** The string literal types of `node`, when its type is made of nothing else (nullish parts aside). */
function typedValues(node) {
  const type = checker.getTypeAtLocation(node);
  const parts = (type.isUnion() ? type.types : [type]).filter(
    (t) => !(t.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null | ts.TypeFlags.Void)),
  );
  if (!parts.length || !parts.every((t) => t.isStringLiteral())) return DYNAMIC;
  return { values: parts.map((t) => ({ value: t.value, node })), dynamic: false };
}

/** Every concatenation of one value of each part; computed at run time when a part is. */
function concatenation(parts, node) {
  if (parts.some((p) => p.dynamic || !p.values.length)) return DYNAMIC;
  let joined = [""];
  for (const p of parts) {
    joined = joined.flatMap((head) => p.values.map((v) => head + v.value));
    if (joined.length > 200) return DYNAMIC;
  }
  return { values: [...new Set(joined)].map((value) => ({ value, node })), dynamic: false };
}

/**
 * The string values `expr` can take: literals, `const` identifiers (in the file or imported),
 * parameters and component props followed to the arguments and JSX attributes of their callers,
 * templates and concatenations of those, and the returns of helper functions. `dynamic` is set
 * when some part is computed at run time; `values` then holds what could be resolved.
 */
export function stringValues(expr, depth = 0, seen = new Set()) {
  expr = unwrap(expr);
  if (!expr) return NOTHING;
  // `seen` holds the path being followed: a cycle is run-time state, a value reached twice is not.
  if (depth > 12 || seen.has(expr)) return DYNAMIC;
  seen.add(expr);
  try {
    return resolveString(expr, depth, seen);
  } finally {
    seen.delete(expr);
  }
}

function resolveString(expr, depth, seen) {
  const next = (e) => stringValues(e, depth + 1, seen);
  if (ts.isStringLiteralLike(expr)) return { values: [{ value: expr.text, node: expr }], dynamic: false };
  if (
    ts.isNumericLiteral(expr) ||
    [ts.SyntaxKind.NullKeyword, ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.FalseKeyword].includes(expr.kind) ||
    (ts.isIdentifier(expr) && expr.text === "undefined")
  )
    return NOTHING;
  if (ts.isConditionalExpression(expr)) return merge([next(expr.whenTrue), next(expr.whenFalse)]);
  if (ts.isBinaryExpression(expr)) {
    const op = expr.operatorToken.kind;
    if (op === ts.SyntaxKind.QuestionQuestionToken || op === ts.SyntaxKind.BarBarToken)
      return merge([next(expr.left), next(expr.right)]);
    if (op === ts.SyntaxKind.PlusToken) return concatenation([next(expr.left), next(expr.right)], expr);
  }
  if (ts.isTemplateExpression(expr)) {
    const text = (t) => ({ values: [{ value: t, node: expr }], dynamic: false });
    return concatenation(
      [text(expr.head.text), ...expr.templateSpans.flatMap((s) => [next(s.expression), text(s.literal.text)])],
      expr,
    );
  }
  let found = DYNAMIC;
  if (ts.isIdentifier(expr)) found = declaredValues(declarationsOf(expr), next, depth, seen);
  else if (ts.isPropertyAccessExpression(expr) && ts.isIdentifier(expr.name))
    found = propertyValues(expr.expression, expr.name.text, next, depth, seen);
  else if (ts.isCallExpression(expr) && ts.isIdentifier(unwrap(expr.expression))) {
    const bodies = declarationsOf(unwrap(expr.expression)).flatMap((d) => {
      if (ts.isFunctionDeclaration(d)) return [d];
      const init = ts.isVariableDeclaration(d) && d.initializer && unwrap(d.initializer);
      return init && (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) ? [init] : [];
    });
    if (bodies.length) found = merge(bodies.flatMap(returnedExpressions).map(next));
  }
  // A value no caller pins down may still have a literal type (`name: GithubHandlerName`).
  if (found.dynamic || !found.values.length) {
    const typed = typedValues(expr);
    if (!typed.dynamic) return typed;
  }
  return found;
}

function declaredValues(declarations, next, depth, seen) {
  if (!declarations.length) return DYNAMIC;
  return merge(
    declarations.map((d) => {
      if (ts.isVariableDeclaration(d) && ts.isIdentifier(d.name))
        return isConst(d) && d.initializer ? next(d.initializer) : DYNAMIC;
      if (ts.isParameter(d) && ts.isIdentifier(d.name)) return argumentValues(d, null, next, depth, seen);
      if (ts.isBindingElement(d) && ts.isIdentifier(d.name) && ts.isObjectBindingPattern(d.parent)) {
        const name = d.propertyName ? d.propertyName.getText() : d.name.text;
        const holder = d.parent.parent;
        if (ts.isVariableDeclaration(holder) && holder.initializer && isConst(holder))
          return propertyValues(holder.initializer, name, next, depth, seen);
        if (ts.isParameter(holder)) return argumentValues(holder, name, next, depth, seen);
      }
      return DYNAMIC;
    }),
  );
}

/** Property `name` of the value `object` evaluates to. */
function propertyValues(object, name, next, depth, seen) {
  object = unwrap(object);
  if (ts.isObjectLiteralExpression(object)) {
    for (const p of object.properties) {
      if (ts.isPropertyAssignment(p) && p.name.getText().replace(/["']/g, "") === name) return next(p.initializer);
      if (ts.isShorthandPropertyAssignment(p) && p.name.text === name) return next(p.name);
    }
    return DYNAMIC;
  }
  if (!ts.isIdentifier(object)) return DYNAMIC;
  return merge(
    declarationsOf(object).map((d) => {
      if (ts.isVariableDeclaration(d) && d.initializer && isConst(d))
        return propertyValues(d.initializer, name, next, depth, seen);
      if (ts.isParameter(d) && ts.isIdentifier(d.name)) return argumentValues(d, name, next, depth, seen);
      return DYNAMIC;
    }),
  );
}

/** The values callers pass for `parameter` (or for its property `name`): call arguments and JSX attributes. */
function argumentValues(parameter, name, next, depth, seen) {
  const fn = parameter.parent;
  if (!ts.isFunctionLike(fn)) return DYNAMIC;
  const index = fn.parameters.indexOf(parameter);
  const results = callsOf(fn).map((call) => {
    const arg = call.arguments[index];
    if (!arg) return parameter.initializer ? next(parameter.initializer) : NOTHING;
    return name === null ? next(arg) : propertyValues(arg, name, next, depth, seen);
  });
  if (index === 0 && name !== null)
    for (const element of jsxOf(fn)) {
      const attributes = element.attributes.properties;
      const attribute = attributes.find((a) => ts.isJsxAttribute(a) && a.name.getText() === name);
      if (!attribute) results.push(attributes.some(ts.isJsxSpreadAttribute) ? DYNAMIC : NOTHING);
      else if (!attribute.initializer) results.push(NOTHING);
      else if (ts.isJsxExpression(attribute.initializer)) results.push(next(attribute.initializer.expression));
      else results.push(next(attribute.initializer));
    }
  return results.length ? merge(results) : DYNAMIC;
}

/**
 * Context keys set (`setContext(key, …)` of `src/platform/context.ts`) and `when` clauses written
 * as something else than a literal at the place that sets or tests them; literals are read from
 * the text. Each use: { kind: "set" | "when", site, values: [{ value, node }], dynamic }.
 */
export function contextKeyUses() {
  const uses = [];
  for (const source of appSources) {
    const visit = (node) => {
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(unwrap(node.expression)) &&
        unwrap(node.expression).text === "setContext" &&
        node.arguments[0] &&
        !ts.isStringLiteralLike(unwrap(node.arguments[0])) &&
        declarationsOf(unwrap(node.expression)).some(
          (d) => rel(d.getSourceFile().fileName) === "src/platform/context.ts",
        )
      )
        uses.push({ kind: "set", site: node, ...stringValues(node.arguments[0]) });
      if (ts.isPropertyAssignment(node) && node.name.getText().replace(/["']/g, "") === "when") {
        const value = unwrap(node.initializer);
        if (
          !ts.isStringLiteralLike(value) &&
          !ts.isArrayLiteralExpression(value) &&
          !ts.isObjectLiteralExpression(value) &&
          !ts.isFunctionLike(value) &&
          !ts.isNumericLiteral(value)
        )
          uses.push({ kind: "when", site: node, ...stringValues(value) });
      }
      if (ts.isShorthandPropertyAssignment(node) && node.name.text === "when") {
        const symbol = checker.getShorthandAssignmentValueSymbol(node);
        const values = declaredValues(symbol?.declarations ?? [], (e) => stringValues(e, 1), 0, new Set());
        uses.push({ kind: "when", site: node, ...values });
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return uses;
}

// ---------------------------------------------------------------------------------------------
// Extension points: every call of an exported `register*` function of the workbench, the platform,
// the editor API and the registries other modules keep (direct, or passed to `.map` / `.forEach`).

export const POINT_FILES = /^src\/(workbench\/|platform\/|editor\/api\.ts|editor\/navigationProviders\.ts)/;
// Registries other modules keep. The inventory lists their registrations; the contribution manifest
// `gen-data.mjs` writes for the GPUI app reads `POINT_FILES` only.
export const REGISTRY_FILES = new RegExp(
  "^src/(editor/relatedProviders\\.ts|editor/lsp/intentions\\.ts|editor/lsp/semantic/languages\\.ts|" +
    "features/vcs/diffViewerProviders\\.ts|features/vcs/commitCheckRegistry\\.ts|ui/pointerDrag\\.ts|ui/icons\\.tsx|" +
    "features/database/(cellActions|tableEditorFocus|valueEditorActions)\\.ts|features/plugins/views/contentTab\\.ts)$",
);
export const isPointFile = (file, registries = true) =>
  POINT_FILES.test(file) || (registries && REGISTRY_FILES.test(file));
export const COVERED_ELSEWHERE = new Set(["registerAction", "registerActions", "registerMessages"]);
export const ID_PROPERTIES = ["id", "kind", "name", "slot", "language", "scheme"];
/** Registries whose first argument is the id itself (`registerLanguageSemantics("go", …)`); a slot
 * argument (`registerMenuSlotItem("pluginTools", …)`) names where items go, not an item. */
export const KEYED_BY_ARGUMENT = new Set(["registerClientCommand", "registerLanguageSemantics"]);

export function pointName(identifier, registries = false) {
  if (!ts.isIdentifier(identifier) || !/^register[A-Z]/.test(identifier.text)) return null;
  const declared = declarationsOf(identifier).some((d) => isPointFile(rel(d.getSourceFile().fileName), registries));
  return declared && !COVERED_ELSEWHERE.has(identifier.text) ? identifier.text : null;
}

export function extensionPoints() {
  /** point → Map(row key → source) */
  const points = new Map();
  const add = (point, objects, callSite, named = []) => {
    const rows = points.get(point) ?? new Map();
    points.set(point, rows);
    const ids = named.length
      ? named
      : objects.flatMap((o) => ID_PROPERTIES.map((p) => property(o, p)).find((v) => v.length) ?? []);
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
        const direct = pointName(callee, true);
        if (direct && node.arguments.length) {
          // The declaration itself (`export const registerX = register(...)`) is not a use.
          const named = KEYED_BY_ARGUMENT.has(direct) ? literals(node.arguments[0]) : [];
          add(direct, named.length ? [] : objectLiterals(node.arguments[0]), node, named);
        } else if (ts.isPropertyAccessExpression(callee) && ["map", "forEach", "flatMap"].includes(callee.name.text)) {
          const passed = node.arguments[0] && pointName(unwrap(node.arguments[0]), true);
          if (passed) add(passed, objectLiterals(callee.expression), node);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return points;
}
